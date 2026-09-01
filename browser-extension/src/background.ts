import { DaemonClient } from "./daemon";
import { handleRequest } from "./messages";
import { isRelayRequest, type WorkerReply } from "./protocol";
import { installGrantRegistration } from "./registration";

type RuntimeMessageSender = { origin?: string };
type Runtime = {
  onMessage: {
    addListener(listener: (message: unknown, sender: RuntimeMessageSender, sendResponse: (response: WorkerReply) => void) => boolean | void): void;
  };
};

export function installBackground(
  runtime: Runtime,
  deps = { permissions: chrome.permissions, daemon: new DaemonClient() },
): void {
  runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!isRelayRequest(message)) return;
    void handleRequest(message.request, sender.origin, deps).then(sendResponse);
    return true;
  });
}

if (typeof chrome !== "undefined") {
  installBackground(chrome.runtime);
  installGrantRegistration({ permissions: chrome.permissions, scripting: chrome.scripting });
}
