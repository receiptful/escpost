import { EscpostError, type ErrorCode } from "../../packages/browser/src/errors";
import type { AccountState } from "./session";

const UNREACHABLE = "Receiptful could not be reached, so HTML could not be rendered.";

export interface StartAuthResult {
  pollToken: string;
  expiresInSeconds: number;
}

export type PollResult =
  | { status: "pending" }


/** The registration payload, in the server's snake_case, so this file is the
 *  only place the two naming conventions meet. */
export interface RegisteredPrinter {
  fingerprint: string;
  /** The canonical catalog profile, as resolved by the server. */
  profile: string;
  /** False when the daemon's profile was unknown and the default was used. */
  profileMatched: boolean;
}

export interface PrinterPayload {
  fingerprint: string;
  strength: "strong" | "weak";
  entry_id: string;
  name: string;
  profile: string;
}

export interface RenderRequest {
  html: string;
  profile: string;
  printerFingerprint: string;
}

export interface RenderResult {
  jobId: number;
  data: string;
  bucket: string;
  signupAllowanceRemaining: number;
  monthlyUsed: number;
}

/** Stateless, like DaemonClient: a recycled worker has nothing to
 *  restore, because the only state is the token in chrome.storage. */
export class ReceiptfulClient {
  readonly #base: string;
  readonly #fetch: typeof fetch;

  constructor(base: string, fetchImpl: typeof fetch = fetch) {
    this.#base = base.replace(/\/$/, "");
    // Bound at the boundary, not at the call site. `fetch` is a WebIDL operation:
    // invoking it as a method of anything but the global -- which `this.#fetch(...)`
    // does -- throws "Illegal invocation". That threw inside the retry loop and
    // surfaced as DAEMON_NOT_RUNNING against a daemon that was running.
    this.#fetch = fetchImpl.bind(globalThis);
  }

  /** Ask the server to email a link. Deliberately returns no credential: the
   *  session token goes to the browser that clicks, never to the caller. */
  async startAuth(email: string): Promise<{ expiresInSeconds: number }> {
    const body = await this.#send<{ expires_in_seconds: number }>(
      "/v1/extension/auth/start",
      { method: "POST", body: { email } },
    );
    return { expiresInSeconds: body.expires_in_seconds };
  }


  /** Fetch the account for a token — and, in doing so, prove the token is
   *  real. The auth bridge calls this before trusting a session handed to it
   *  by the verify page. */
  async account(token: string): Promise<AccountState> {
    return toAccount(await this.#send<RawAccount>("/v1/extension/account", { method: "GET", token }));
  }

  async signOut(token: string): Promise<void> {
    await this.#send<null>("/v1/extension/auth/signout", { method: "POST", token });
  }

  /** Register, and return the profiles the SERVER resolved.
   *
   *  The daemon's profile vocabulary is not the catalog's — printers.toml is
   *  local and merchant-edited — so the canonical name comes back from here
   *  and is what /render will accept. */
  async registerPrinters(token: string, printers: PrinterPayload[]): Promise<RegisteredPrinter[]> {
    const body = await this.#send<{
      printers: { fingerprint: string; profile: string; profile_matched?: boolean }[];
    }>("/v1/extension/printers", { method: "POST", token, body: { printers } });
    return (body?.printers ?? []).map((p) => ({
      fingerprint: p.fingerprint,
      profile: p.profile,
      profileMatched: p.profile_matched !== false,
    }));
  }

  async render(token: string, request: RenderRequest): Promise<RenderResult> {
    const body = await this.#send<{
      job_id: number;
      data: string;
      bucket: string;
      signup_allowance_remaining: number;
      monthly_used: number;
    }>("/v1/extension/render", {
      method: "POST",
      token,
      body: { html: request.html, profile: request.profile, printer_fingerprint: request.printerFingerprint },
    });
    return {
      jobId: body.job_id,
      data: body.data,
      bucket: body.bucket,
      signupAllowanceRemaining: body.signup_allowance_remaining,
      monthlyUsed: body.monthly_used,
    };
  }

  async reportResult(token: string, jobId: number, status: "completed" | "failed", message?: string): Promise<void> {
    await this.#send<null>(`/v1/extension/jobs/${jobId}/result`, {
      method: "POST",
      token,
      body: message === undefined ? { status } : { status, message },
    });
  }

  async #send<T>(path: string, options: { method: string; token?: string; body?: unknown }): Promise<T> {
    const headers: Record<string, string> = {};
    if (options.token) headers["Authorization"] = `Bearer ${options.token}`;
    if (options.body !== undefined) headers["Content-Type"] = "application/json";

    let response: Response;
    try {
      response = await this.#fetch(this.#base + path, {
        method: options.method,
        headers,
        ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
      });
    } catch {
      // Offline, DNS down, or the machine is on a till network with no
      // route out. RENDER_UNAVAILABLE is the code whose message already
      // says raw printing is unaffected.
      throw new EscpostError("RENDER_UNAVAILABLE", UNREACHABLE);
    }

    if (response.status === 204) return null as T;
    if (response.ok) return (await response.json()) as T;
    throw await toError(response);
  }
}

interface RawAccount {
  email: string;
  org_id: string;
  project_id: string;
  signup_allowance_remaining: number;
  monthly_used: number;
  monthly_limit: number;
  has_paid_access: boolean;
}

function toAccount(raw: RawAccount): AccountState {
  return {
    email: raw.email,
    orgId: raw.org_id,
    projectId: raw.project_id,
    signupAllowanceRemaining: raw.signup_allowance_remaining,
    monthlyUsed: raw.monthly_used,
    monthlyLimit: raw.monthly_limit,
    hasPaidAccess: raw.has_paid_access,
  };
}

interface ValidationIssue {
  loc?: (string | number)[];
  msg?: string;
}

async function toError(response: Response): Promise<EscpostError> {
  let code: ErrorCode = "PRINT_FAILED";
  let message = `Receiptful returned ${response.status}.`;
  try {
    const body = (await response.json()) as {
      detail?: { code?: string; message?: string } | string | ValidationIssue[];
    };
    // FastAPI's 422 body is an ARRAY of issues, not this surface's
    // {code,message} envelope. Checked FIRST because `typeof [] === "object"`,
    // so the object branch below swallowed it and left the caller with a bare
    // "Receiptful returned 422." — which is what made an ordinary profile
    // mismatch impossible to diagnose.
    if (Array.isArray(body.detail)) {
      const named = body.detail
        .map((issue) => {
          const field = Array.isArray(issue.loc) ? issue.loc.filter((p) => p !== "body").join(".") : "";
          const reason = (issue.msg ?? "is not valid").replace(/^Value error,\s*/, "");
          return field ? `${field}: ${reason}` : reason;
        })
        .join("; ");
      if (named) message = `Receiptful rejected this request: ${named}`;
      code = "UNSUPPORTED_FORMAT";
    } else if (typeof body.detail === "object" && body.detail !== null) {
      if (body.detail.message) message = body.detail.message;
      if (body.detail.code) code = body.detail.code as ErrorCode;
    } else if (typeof body.detail === "string") {
      // FastAPI's own validation errors, which carry no typed code.
      message = body.detail;
      code = "UNSUPPORTED_FORMAT";
    }
  } catch {
    // A non-JSON error body is not worth reporting over the real failure.
  }
  return new EscpostError(code, message);
}
