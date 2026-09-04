import type { PageMessage } from "./protocol";
import type { PageWindow } from "./transport";

type FrameWindow = { postMessage(message: PageMessage, targetOrigin: string): void };
type Frame = {
  contentWindow: FrameWindow | null;
  src: string;
  hidden: boolean;
  tabIndex: number;
  addEventListener(type: "load", listener: () => void): void;
};
type HostWindow = { addEventListener(type: "message", listener: (event: MessageEvent) => void): void };
type FrameDocument = {
  createElement(name: "iframe"): Frame;
  documentElement: { append(node: Frame): void };
};

export class IframePage implements PageWindow {
  private readonly origin: string;
  private frame: Frame | undefined;
  private loaded = false;
  private readonly queued: PageMessage[] = [];

  constructor(
    extensionId: string,
    private readonly host: HostWindow = window,
    private readonly document: FrameDocument = globalThis.document as unknown as FrameDocument,
  ) {
    this.origin = `chrome-extension://${extensionId}`;
  }

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    const frame = this.ensureFrame();
    this.host.addEventListener("message", (event) => {
      if (event.source !== frame.contentWindow || event.origin !== this.origin) return;
      listener({ source: this, data: event.data } as unknown as MessageEvent);
    });
  }

  postMessage(message: PageMessage): void {
    const frame = this.ensureFrame();
    if (!this.loaded) {
      this.queued.push(message);
      return;
    }
    frame.contentWindow?.postMessage(message, this.origin);
  }

  private ensureFrame(): Frame {
    if (this.frame !== undefined) return this.frame;
    const frame = this.document.createElement("iframe");
    frame.src = `${this.origin}/bridge.html`;
    frame.hidden = true;
    frame.tabIndex = -1;
    frame.addEventListener("load", () => {
      this.loaded = true;
      for (const message of this.queued.splice(0)) frame.contentWindow?.postMessage(message, this.origin);
    });
    this.document.documentElement.append(frame);
    this.frame = frame;
    return frame;
  }
}
