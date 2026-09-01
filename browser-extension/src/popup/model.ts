export type StatusTone = "ok" | "warning" | "error" | "neutral";

export type PopupStatus = { tone: StatusTone; label: string };

export type PopupAction = { kind: "grant" | "revoke"; label: string };

export type PopupView = {
  origin: string;
  permission: PopupStatus;
  daemon: PopupStatus;
  primaryAction: PopupAction | null;
  guidance: string;
  error: string | null;
};

export type PopupModelInput = {
  origin: string | null;
  grant: "present" | "absent" | "unknown";
  relay: "loaded" | "missing" | "unknown";
  daemon: "running" | "unavailable" | "unknown";
  error?: string | null;
};

export function buildPopupView(input: PopupModelInput): PopupView {
  const daemon = daemonStatus(input.daemon);
  if (input.origin === null) {
    return {
      origin: "No supported website",
      permission: { tone: "error", label: "This page cannot be granted access" },
      daemon,
      primaryAction: null,
      guidance: "Open an HTTP or HTTPS website to allow raw printing.",
      error: input.error ?? null,
    };
  }
  if (input.grant === "unknown") {
    return {
      origin: input.origin,
      permission: { tone: "error", label: "Could not verify access" },
      daemon,
      primaryAction: null,
      guidance: "Close and reopen the popup to try again.",
      error: input.error ?? null,
    };
  }
  if (input.grant === "absent") {
    return {
      origin: input.origin,
      permission: { tone: "warning", label: "Not allowed" },
      daemon,
      primaryAction: { kind: "grant", label: "Allow this site" },
      guidance: "Allow this site to use raw printing through escpost.",
      error: input.error ?? null,
    };
  }
  if (input.relay !== "loaded") {
    return {
      origin: input.origin,
      permission: { tone: "ok", label: "Allowed" },
      daemon,
      primaryAction: { kind: "revoke", label: "Remove access" },
      guidance: "Reload this page to load the escpost raw-printing relay.",
      error: input.error ?? null,
    };
  }
  return {
    origin: input.origin,
    permission: { tone: "ok", label: "Allowed" },
    daemon,
    primaryAction: { kind: "revoke", label: "Remove access" },
    guidance: "Raw printing is ready for this page.",
    error: input.error ?? null,
  };
}

function daemonStatus(state: PopupModelInput["daemon"]): PopupStatus {
  switch (state) {
    case "running":
      return { tone: "ok", label: "escpost is running" };
    case "unavailable":
      return { tone: "error", label: "escpost is unavailable" };
    case "unknown":
      return { tone: "neutral", label: "escpost status is unknown" };
  }
}
