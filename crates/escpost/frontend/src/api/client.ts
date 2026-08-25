import type {
  AddPrinterBody,
  AddPrinterResponse,
  ApiErrorEnvelope,
  CurrentJobResponse,
  DiscoveryNetworksResponse,
  PrintersResponse,
  ProfilesResponse,
} from "./types";

export class ApiRequestError extends Error {
  readonly kind = "api";

  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiRequestError";
  }
}

export class NetworkRequestError extends Error {
  readonly kind = "network";

  constructor(message = "Unable to reach the ESCPost server.") {
    super(message);
    this.name = "NetworkRequestError";
  }
}

function isErrorEnvelope(value: unknown): value is ApiErrorEnvelope {
  if (!value || typeof value !== "object" || !("error" in value)) {
    return false;
  }
  const error = value.error;
  return Boolean(
    error
      && typeof error === "object"
      && "code" in error
      && typeof error.code === "string"
      && "message" in error
      && typeof error.message === "string",
  );
}

// Shared by every verb: fetch, translate a rejected/aborted fetch into
// `NetworkRequestError`, then decode the JSON envelope the same way whether
// the request was a GET or a POST. `requestJson` and `postJson` differ only
// in the `RequestInit` they build.
async function request<T>(path: string, init: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, init);
  } catch (error) {
    if (init.signal?.aborted || (error instanceof DOMException && error.name === "AbortError")) {
      throw error;
    }
    throw new NetworkRequestError();
  }

  const contentType = response.headers.get("content-type") ?? "";
  let body: unknown;
  if (contentType.includes("application/json")) {
    try {
      body = await response.json();
    } catch {
      throw new ApiRequestError(response.status, "invalid_json", "The server returned invalid JSON.");
    }
  } else {
    throw new ApiRequestError(
      response.status,
      "unexpected_response",
      "The server returned an unexpected response.",
    );
  }

  if (!response.ok) {
    if (isErrorEnvelope(body)) {
      throw new ApiRequestError(response.status, body.error.code, body.error.message);
    }
    throw new ApiRequestError(response.status, "request_failed", "The request failed.");
  }

  return body as T;
}

export function requestJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  return request<T>(path, {
    headers: { Accept: "application/json" },
    cache: "no-store",
    signal,
  });
}

function postJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  return request<T>(path, {
    method: "POST",
    headers: { Accept: "application/json", "Content-Type": "application/json" },
    cache: "no-store",
    body: JSON.stringify(body),
    signal,
  });
}

export function getPrinters(transport?: "usb" | "network", signal?: AbortSignal) {
  const query = transport ? `?transport=${encodeURIComponent(transport)}` : "";
  return requestJson<PrintersResponse>(`/api/printers/list${query}`, signal);
}

export function getProfiles(signal?: AbortSignal) {
  return requestJson<ProfilesResponse>("/api/profiles/list", signal);
}

export function getCurrentJob(signal?: AbortSignal) {
  return requestJson<CurrentJobResponse>("/api/jobs/current", signal);
}

export function getDiscoveryNetworks(signal?: AbortSignal) {
  return requestJson<DiscoveryNetworksResponse>("/api/printers/discover/networks", signal);
}

export function addPrinter(body: AddPrinterBody, signal?: AbortSignal) {
  return postJson<AddPrinterResponse>("/api/printers/add", body, signal);
}
