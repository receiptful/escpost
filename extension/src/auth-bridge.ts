/**
 * Carries the session token from the verify page to the service worker.
 *
 * Sign-in is link-only: the server hands the token to the browser that clicked
 * the link, in the body of the success page. This content script is how that
 * page reaches the extension. It exists because an extension cannot receive a
 * redirect — which is the only reason the old design polled, and the poll was
 * the hole: it answered "done yet?" with a session token to whoever held a
 * poll token.
 *
 * The token is read from a JSON island in the DOM, never from a postMessage:
 * any script on the page could forge one of those. And the script is scoped by
 * the manifest to the verify path on our own origin, so no other page runs it.
 */

const ISLAND = "script#escpost-session";

/** Set on <html> once the worker has the token, so the page can tell whether
 *  escpost is installed in THIS browser — the cross-device case. */
const ACK_ATTRIBUTE = "data-escpost-ack";

export async function handOffSession(): Promise<void> {
  const island = document.querySelector(ISLAND);
  if (island === null) return;

  let token: unknown;
  try {
    token = (JSON.parse(island.textContent ?? "") as { token?: unknown }).token;
  } catch {
    // A page of ours that does not parse is not one to act on.
    return;
  }
  if (typeof token !== "string" || token === "") return;

  try {
    const reply = (await chrome.runtime.sendMessage({ op: "auth.bridge", payload: { token } })) as
      | { ok?: boolean }
      | undefined;
    // Acknowledge only on success. A silent failure here would leave the page
    // claiming a sign-in that never reached the extension.
    if (reply?.ok === true) document.documentElement.setAttribute(ACK_ATTRIBUTE, "1");
  } catch {
    // The worker is gone or the extension was just reloaded. The page's own
    // timeout then tells the user where to open the link.
  }
}

if (typeof chrome !== "undefined" && chrome.runtime !== undefined) void handOffSession();
