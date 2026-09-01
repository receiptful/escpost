import { DaemonClient } from "./daemon";
import { installInventoryStreams } from "./inventory-stream";
import { handleRequest, type RequestDependencies } from "./messages";
import { isRelayRequest, type WorkerReply } from "./protocol";
import { installGrantRegistration } from "./registration";

type RuntimeMessageSender = { origin?: string };
type Runtime = {
  onMessage: {
    addListener(listener: (message: unknown, sender: RuntimeMessageSender, sendResponse: (response: WorkerReply) => void) => boolean | void): void;
  };
};
type RequestHandler = (request: unknown, senderOrigin: string | undefined, deps: RequestDependencies) => Promise<WorkerReply>;

export function installBackground(
  runtime: Runtime,
  deps: RequestDependencies = { permissions: chrome.permissions, daemon: new DaemonClient() },
  requestHandler: RequestHandler = handleRequest,
): void {
  runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!isRelayRequest(message)) return;
    let responded = false;
    const respond = (reply: WorkerReply) => {
      if (responded) return;
      responded = true;
      sendResponse(reply);
    };
    void requestHandler(message.request, sender.origin, deps).then(
      respond,
      () => respond({ ok: false, error: { code: "DAEMON_UNAVAILABLE", message: "The local ESCPost daemon is unavailable." } }),
    );
    return true;
  });
}

if (typeof chrome !== "undefined") {
  const daemon = new DaemonClient();
  installBackground(chrome.runtime, { permissions: chrome.permissions, daemon });
  installInventoryStreams(chrome.runtime, { permissions: chrome.permissions, daemon });
  installGrantRegistration({ permissions: chrome.permissions, scripting: chrome.scripting });
}
