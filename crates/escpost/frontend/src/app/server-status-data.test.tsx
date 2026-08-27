import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, render, screen, waitFor } from "@testing-library/preact";
import { ServerStatusProvider, useServerStatus } from "./server-status-data";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  closed = false;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(_url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    const existing = this.listeners.get(name) ?? [];
    existing.push(handler);
    this.listeners.set(name, existing);
  }

  close() {
    this.closed = true;
  }

  emit(name: string, data?: unknown) {
    const event = data === undefined ? new Event(name) : new MessageEvent(name, { data: JSON.stringify(data) });
    for (const handler of this.listeners.get(name) ?? []) {
      handler(event);
    }
  }
}

const originalEventSource = globalThis.EventSource;

function Probe() {
  const resource = useServerStatus();
  return <p>{`${resource.phase}:${resource.snapshot?.jobs_processed ?? "null"}:${resource.error?.message ?? "none"}`}</p>;
}

function renderProvider(retryDelayMs?: number) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  return render(
    <ServerStatusProvider retryDelayMs={retryDelayMs}><Probe /></ServerStatusProvider>,
  );
}

afterEach(() => {
  cleanup();
  globalThis.EventSource = originalEventSource;
});

describe("ServerStatusProvider", () => {
  test("moves from checking through reconnecting status states", () => {
    renderProvider();
    const source = FakeEventSource.instances[0]!;

    expect(screen.getByText("checking:null:none")).toBeTruthy();
    act(() => {
      source.emit("message", { virtual_printer: null, jobs_processed: 4, config_path: "/tmp/printers.toml" });
    });
    expect(screen.getByText("ready:4:none")).toBeTruthy();
    act(() => {
      source.emit("error");
    });
    expect(screen.getByText("disconnected:4:Unable to reach the ESCPost server.")).toBeTruthy();
    act(() => {
      source.emit("message", { virtual_printer: null, jobs_processed: 5, config_path: "/tmp/printers.toml" });
    });
    expect(screen.getByText("ready:5:none")).toBeTruthy();
  });

  test("closes the persistent source when unmounted", () => {
    const view = renderProvider();
    const source = FakeEventSource.instances[0]!;

    view.unmount();

    expect(source.closed).toBe(true);
  });

  test("retains the last snapshot while malformed data disconnects the resource", () => {
    renderProvider();
    const source = FakeEventSource.instances[0]!;
    act(() => {
      source.emit("message", { virtual_printer: null, jobs_processed: 4, config_path: "/tmp/printers.toml" });
    });
    act(() => {
      source.emit("message", { virtual_printer: null, jobs_processed: -1, config_path: "/tmp/printers.toml" });
    });

    expect(screen.getByText("disconnected:4:The server returned invalid status data.")).toBeTruthy();
    expect(source.closed).toBe(false);
  });

  // A browser reopens a dropped stream on its own, but gives up for good when
  // the answer is not an event stream at all, which is what a proxy sends
  // while the server it stands in front of restarts. The status would then
  // stay unavailable until the reader reloaded the page.
  test("opens the stream again after the browser gives up on it", async () => {
    renderProvider(0);
    const first = FakeEventSource.instances[0]!;
    act(() => {
      first.emit("message", { virtual_printer: null, jobs_processed: 4, config_path: "/tmp/printers.toml" });
    });

    act(() => {
      first.emit("error");
    });
    expect(screen.getByText("disconnected:4:Unable to reach the ESCPost server.")).toBeTruthy();

    await waitFor(() => expect(FakeEventSource.instances).toHaveLength(2));
    expect(first.closed).toBe(true);
    act(() => {
      FakeEventSource.instances[1]!.emit("message", {
        virtual_printer: null,
        jobs_processed: 6,
        config_path: "/tmp/printers.toml",
      });
    });

    expect(screen.getByText("ready:6:none")).toBeTruthy();
  });

  test("stops trying once the reader leaves the page", async () => {
    const view = renderProvider(0);
    act(() => {
      FakeEventSource.instances[0]!.emit("error");
    });
    view.unmount();

    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(FakeEventSource.instances).toHaveLength(1);
  });
});
