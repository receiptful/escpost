import { installBackground } from "../background";
import { DaemonClient } from "../daemon";
import { installInventoryStreams } from "../inventory-stream";
import { installGrantRegistration } from "../registration";
import { FirefoxOriginGrants } from "./grants";

const daemon = new DaemonClient();
const grants = new FirefoxOriginGrants(chrome.permissions);

installBackground(chrome.runtime, { grants, daemon });
installInventoryStreams(chrome.runtime, { grants, daemon });
installGrantRegistration({ permissions: chrome.permissions, scripting: chrome.scripting });
