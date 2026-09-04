import { DaemonClient } from "../daemon";
import { installPopup } from "../popup/popup";
import { ChromeOriginGrants } from "./grants";

const daemon = new DaemonClient();

installPopup({
  document,
  tabs: chrome.tabs,
  grants: new ChromeOriginGrants(chrome.storage.local, chrome.storage.onChanged),
  afterGrantChange: async () => {},
  async probe() {
    try {
      const running = await daemon.health();
      return { relay: "loaded", daemon: running ? "running" : "unavailable", error: null };
    } catch {
      return { relay: "loaded", daemon: "unavailable", error: null };
    }
  },
});
