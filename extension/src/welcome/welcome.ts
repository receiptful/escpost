export type WelcomePhase =
  | { kind: "offer" }
  | { kind: "sent"; email: string }
  /** The link expired before it was opened. Only a new link helps. */
  | { kind: "expired" }
  | { kind: "signed-in"; email: string }
  | { kind: "confirming-skip" }
  | { kind: "skipped" };

/**
 * What the machine actually reports, checked when the page opens.
 *
 * The page used to claim printing worked before asking anything, which is a
 * lie on the one run where it matters: a fresh install with escpost not yet
 * started.
 */
export type Readiness =
  | { kind: "checking" }
  | { kind: "ready"; printer: string }
  | { kind: "no-printer" }
  | { kind: "not-running" };

export interface WelcomeAction {
  label: string;
  /** `test-print` sends a receipt; `recheck` runs the readiness check again. */
  kind: "test-print" | "recheck";
}

export interface WelcomeView {
  heading: string;
  body: string;
  worksNow: string[];
  unlocks: string[];
  showEmailField: boolean;
  showSkip: boolean;
  /** Stated once, on the skip confirmation, and nowhere else. */
  warning: string | null;
  /** Always null. Present as a field so that anyone adding a timed
   *  re-prompt has to change a value a test is watching. */
  repromptAfterMs: null;
  /** The one thing to do next, or null when the page is waiting on a check. */
  action: WelcomeAction | null;
  /** Something to type, shown only when typing it is the way forward. */
  command: string | null;
  /** Mirrors the popup's pill so both surfaces report state the same way. */
  status: { label: string; tone: "ok" | "warn" | "out" | "mute" } | null;
}

const WORKS_NOW = [
  "Raw ESC/POS printing is set up and unlimited. No account, no certificate, no dialog on every print.",
  "It works offline, and nothing you print this way reaches us.",
];

const UNLOCKS = [
  "HTML receipts, rendered into real printer text rather than an image.",
  "200 free receipts to start with, then 20 a month on the free plan.",
];

/** Every copy decision on this screen, in one pure function, because they
 *  are requirements about wording and a requirement about wording that is
 *  only visible in a screenshot is a requirement nothing enforces. */
export function describeWelcome(
  phase: WelcomePhase,
  readiness: Readiness = { kind: "checking" },
): WelcomeView {
  const base = {
    worksNow: WORKS_NOW,
    unlocks: UNLOCKS,
    warning: null,
    repromptAfterMs: null,
    action: null,
    command: null,
    status: null,
  } as const;

  switch (phase.kind) {
    case "offer":
      return { ...base, ...describeReadiness(readiness), showEmailField: true, showSkip: true };

    case "sent":
      return {
        ...base,
        heading: "Check your email",
        body:
          `We sent a sign-in link to ${phase.email}. Open it in this browser. ` +
          "The link signs in whichever browser opens it, so opening it on your phone " +
          "signs in your phone instead.",
        showEmailField: false,
        showSkip: true,
      };

    case "expired":
      return {
        ...base,
        heading: "This link has expired",
        body:
          "Sign-in links are short-lived. Enter your email again and we will send a new one. " +
          "Nothing you have set up has changed.",
        showEmailField: true,
        showSkip: true,
      };

    case "signed-in":
      return {
        ...base,
        heading: "HTML receipts are ready",
        body:
          `Signed in as ${phase.email}. You can close this tab. Printing carries on as before, ` +
          "with HTML available too.",
        showEmailField: false,
        showSkip: false,
      };

    case "confirming-skip":
      return {
        ...base,
        heading: "Skip for now?",
        body: "Nothing you have set up changes.",
        showEmailField: false,
        showSkip: true,
        warning:
          "You keep unlimited raw ESC/POS printing. HTML receipts stay unavailable and the " +
          "200 free receipts go unclaimed. You can add an email later from the extension.",
      };

    case "skipped": {
      // "All set" with nothing to do is a dead end. When the path is ready,
      // the way to prove it is to print, so that is the one thing offered.
      const settled = describeReadiness(readiness);
      return {
        ...base,
        heading: "All set",
        body:
          "Raw printing is unlimited and needs nothing further. To add HTML receipts later, " +
          "open the escpost extension. The offer stays there.",
        action: settled.action,
        command: settled.command,
        status: settled.status,
        showEmailField: false,
        showSkip: false,
      };
    }
  }
}


function paint(view: WelcomeView): void {
  setText("#heading", view.heading);
  setText("#body", view.body);
  setList("#works-now", view.worksNow);
  setList("#unlocks", view.unlocks);
  toggle("#signup", view.showEmailField);
  toggle("#skip", view.showSkip);
  toggle("#warning", view.warning !== null);
  if (view.warning !== null) setText("#warning", view.warning);

  toggle("#status", view.status !== null);
  if (view.status !== null) {
    const pill = document.querySelector("#status");
    if (pill) {
      pill.className = `pill ${view.status.tone}`;
      pill.textContent = view.status.label;
    }
  }

  toggle("#command", view.command !== null);
  if (view.command !== null) setText("#command", view.command);

  toggle("#action", view.action !== null);
  if (view.action !== null) setText("#action", view.action.label);
}

/** A receipt short enough to waste no paper and complete enough to prove the
 *  path: initialise, print, feed clear of the head, cut. */
const TEST_RECEIPT = "\x1b@\x1b!\x30escpost\x1b!\x00\nTest receipt\nPrinting works.\n\n\n\x1dV\x00";

async function detectReadiness(): Promise<Readiness> {
  const reachable = await chrome.runtime.sendMessage({ op: "daemon.available", payload: undefined });
  if (!reachable?.ok || reachable.data !== true) return { kind: "not-running" };

  const printers = await chrome.runtime.sendMessage({ op: "printers.list", payload: undefined });
  const list = (printers?.ok ? printers.data : []) as Array<{ id: string; name: string }>;
  const first = list[0];
  if (first === undefined) return { kind: "no-printer" };
  return { kind: "ready", printer: first.name };
}

async function main(): Promise<void> {
  let email = "";
  let phase: WelcomePhase = { kind: "offer" };
  let readiness: Readiness = { kind: "checking" };

  const repaint = () => paint(describeWelcome(phase, readiness));

  const check = async (): Promise<void> => {
    readiness = { kind: "checking" };
    repaint();
    readiness = await detectReadiness();
    repaint();
  };

  repaint();
  void check();

  document.querySelector("#action")?.addEventListener("click", async () => {
    const view = describeWelcome(phase, readiness);
    if (view.action?.kind === "recheck") {
      await check();
      return;
    }
    if (view.action?.kind !== "test-print" || readiness.kind !== "ready") return;

    setText("#result", "Printing.");
    toggle("#result", true);
    const answer = await chrome.runtime.sendMessage({
      op: "print",
      payload: { printer: readiness.printer, data: btoa(TEST_RECEIPT) },
    });
    setText(
      "#result",
      answer?.ok === true
        ? "Printed. Check your printer."
        : (answer?.error?.message ?? "The print did not go through."),
    );
  });

  document.querySelector("#signup")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    email = (document.querySelector("#email") as HTMLInputElement).value.trim();
    const answer = await chrome.runtime.sendMessage({ op: "auth.start", payload: { email } });
    if (!answer?.ok) {
      paint({ ...describeWelcome(phase, readiness), body: answer?.error?.message ?? "Sign-in failed." });
      return;
    }
    // Nothing comes back but a lifetime. The session token goes to whichever
    // browser opens the link, and the auth-bridge content script on the verify
    // page hands it to the worker — there is nothing here to wait for.
    phase = { kind: "sent", email };
    repaint();
  });

  // The bridge signs in from the verify tab, which may be this tab's sibling.
  // Reflect that here rather than leaving "check your email" on screen forever.
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== "local" || changes["account"] === undefined) return;
    const next = changes["account"].newValue as { email?: string; verified?: boolean } | undefined;
    if (next?.verified) {
      phase = { kind: "signed-in", email: next.email ?? email };
      repaint();
    }
  });

  document.querySelector("#skip")?.addEventListener("click", async () => {
    // the warning is a confirmation, not a nag, and it is shown once.
    if (document.querySelector("#warning")?.hasAttribute("hidden")) {
      phase = { kind: "confirming-skip" };
      repaint();
      return;
    }
    await chrome.storage.local.set({ onboardingSkipped: true });
    phase = { kind: "skipped" };
    repaint();
  });
}

function setText(selector: string, value: string): void {
  const element = document.querySelector(selector);
  if (element) element.textContent = value;
}

function setList(selector: string, values: string[]): void {
  const element = document.querySelector(selector);
  if (!element) return;
  element.replaceChildren(
    ...values.map((value) => {
      const item = document.createElement("li");
      item.textContent = value;
      return item;
    }),
  );
}

function toggle(selector: string, visible: boolean): void {
  document.querySelector(selector)?.toggleAttribute("hidden", !visible);
}

if (typeof document !== "undefined" && document.querySelector("#heading")) void main();

/** Heading, body and next action for whatever the check found. */
function describeReadiness(readiness: Readiness): Pick<WelcomeView, "heading" | "body" | "action" | "command" | "status"> {
  switch (readiness.kind) {
    case "checking":
      return {
        heading: "The escpost extension is installed",
        body: "Checking whether escpost is running on this machine.",
        action: null,
        command: null,
        status: { label: "Checking", tone: "mute" },
      };

    case "ready":
      return {
        heading: "Ready to print",
        body:
          `escpost is running and ${readiness.printer} is connected. ` +
          "Print a test receipt to see the whole path work.",
        action: { label: "Print a test receipt", kind: "test-print" },
        command: null,
        status: { label: "Ready", tone: "ok" },
      };

    case "no-printer":
      return {
        heading: "escpost is running, no printer yet",
        body: "Add a printer on this machine, then check again.",
        action: { label: "Check again", kind: "recheck" },
        command: "escpost printers add",
        status: { label: "No printer", tone: "warn" },
      };

    case "not-running":
      return {
        heading: "Start escpost to print",
        body:
          "The extension is installed. It reaches your printer through escpost, " +
          "which is not answering on this machine yet.",
        action: { label: "Check again", kind: "recheck" },
        command: null,
        status: { label: "Not running", tone: "out" },
      };
  }
}
