import { EscpostError, isEscpostError } from "./errors";
import { EXT_SOURCE, PAGE_SOURCE, PROTOCOL_VERSION, type ExtMessage } from "./protocol";

const DEFAULT_TIMEOUT_MS = 2000;

const NOT_INSTALLED =
  "The escpost extension is not installed, or this site has not been granted access. " +
  "Install it from the Chrome Web Store, then reload this page.";

/**
 * Only `id` survives the round trip, so it is the only thing that can tell two
 * copies of this package apart. A page that loads the library twice, bundled
 * once and script-tagged once, would otherwise have both copies counting from
 * 1, both listening on window, and each resolving the other's replies with the
 * wrong caller's data. Starting each copy in a random block makes that
 * collision vanishingly unlikely without changing the message shape.
 */
const ID_BLOCK = 2 ** 20;
let nextId = Math.floor(Math.random() * 2 ** 31) * ID_BLOCK;

const pending = new Map<
  number,
  { resolve: (value: unknown) => void; reject: (reason: unknown) => void; timer: ReturnType<typeof setTimeout> }
>();
let listening = false;

/** The protocol version travels outbound only: the worker rejects a request it
 *  cannot speak, so a reply is by definition already agreed. */
export function request<T = unknown>(op: string, payload: unknown, options: { timeoutMs?: number } = {}): Promise<T> {
  ensureListening();
  const id = ++nextId;
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;

  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(timeoutError(op, timeoutMs));
    }, timeoutMs);

    pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
    window.postMessage({ source: PAGE_SOURCE, id, protocol: PROTOCOL_VERSION, op, payload }, "*");
  });
}

function ensureListening(): void {
  if (listening) return;
  listening = true;
  window.addEventListener("message", (event: MessageEvent) => {
    if (event.source !== window) return;
    const message = event.data as ExtMessage | undefined;
    if (message?.source !== EXT_SOURCE || typeof message.id !== "number") return;
    const entry = pending.get(message.id);
    if (!entry) return;
    clearTimeout(entry.timer);
    pending.delete(message.id);
    if (message.ok) {
      entry.resolve(message.data);
      return;
    }
    const failure = message.error;
    entry.reject(
      isEscpostError(failure)
        ? new EscpostError(failure.code, failure.message)
        : new EscpostError("PRINT_FAILED", failure?.message ?? "The extension returned an unrecognised error."),
    );
  });
}

/**
 * A relay that is present answers immediately, even to report a failure, so
 * silence past the short default means nothing is listening. A caller-supplied
 * budget is different: it is only ever exceeded while the extension is working,
 * and reporting that as "not installed" sends an operator off to reinstall a
 * working extension while their receipt never prints.
 */
function timeoutError(op: string, timeoutMs: number): EscpostError {
  if (timeoutMs <= DEFAULT_TIMEOUT_MS) return new EscpostError("EXTENSION_NOT_INSTALLED", NOT_INSTALLED);
  return new EscpostError("PRINT_FAILED", `The extension did not finish "${op}" within ${timeoutMs} ms.`);
}
