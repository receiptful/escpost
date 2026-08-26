import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { AppDataProvider, useAppData } from "./data";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  closed = false;
  constructor(readonly url: string) { FakeEventSource.instances.push(this); }
  addEventListener(_name: string, _handler: (event: Event) => void) {}
  close() { this.closed = true; }
}
const originalEventSource = globalThis.EventSource;

function Probe() {
  const { startScan, scan } = useAppData();
  return <><button type="button" onClick={() => startScan({ usb: true, network: true, subnets: [] })}>Scan</button><p>{scan.phase}</p></>;
}

afterEach(() => { cleanup(); globalThis.EventSource = originalEventSource; });

describe("AppDataProvider", () => {
  test("keeps discovery-stream ownership separate from printer inventory", () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    render(<AppDataProvider><Probe /></AppDataProvider>);
    act(() => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    act(() => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    expect(FakeEventSource.instances).toHaveLength(2);
    expect(FakeEventSource.instances[0]?.closed).toBe(true);
    expect(screen.getByText("running")).toBeTruthy();
  });
});
