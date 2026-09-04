import { FirefoxOriginGrants } from "./grants";
import { installPopup } from "../popup/popup";
import { registerGrantedRelay } from "../registration";
import { probeRelayStatus } from "../ui/status";

const registration = { permissions: chrome.permissions, scripting: chrome.scripting };

installPopup({
  document,
  tabs: chrome.tabs,
  grants: new FirefoxOriginGrants(chrome.permissions),
  afterGrantChange: () => registerGrantedRelay(registration),
  probe: (tabId) => probeRelayStatus(tabId, chrome.tabs),
});
