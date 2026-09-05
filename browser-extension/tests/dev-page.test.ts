// @vitest-environment happy-dom

import { beforeEach, expect, test, vi } from "vitest";
import { Window } from "happy-dom";

const testWindow = new Window();
Object.assign(globalThis, { window: testWindow, document: testWindow.document, Event: testWindow.Event });

// @ts-expect-error The manual page stays a plain browser module for static hosting.
const { startSdkPage } = await import("../dev/sdk-page/page.js");

type Snapshot = {
  updatedAt: string;
  warning: string | null;
  printers: Array<{
    name: string;
    transport: "network" | "usb";
    availability: "connected" | "unavailable";
    profile: string | null;
    connection: { type: "network"; host: string; port: number } | {
      type: "usb";
      vendorId: number;
      productId: number;
      bus: string | null;
      address: number | null;
      manufacturer: string | null;
      product: string | null;
      serialNumber: string | null;
      interfaceNumber: number;
      outEndpoints: number[];
      inEndpoints: number[];
    };
  }>;
};

type Sdk = {
  isAvailable(): Promise<boolean>;
  printers: {
    list(): Promise<Snapshot>;
    subscribe(onSnapshot: (snapshot: Snapshot) => void, options: { onError?: (error: Error) => void }): () => void;
  };
  print(request: { printer: string; data: Uint8Array | string }): Promise<{ jobId: string }>;
};

const initialSnapshot: Snapshot = {
  updatedAt: "2026-09-01T12:00:00Z",
  warning: "Network discovery is still running.",
  printers: [
    {
      name: "counter",
      transport: "network",
      availability: "connected",
      profile: null,
      connection: { type: "network", host: "192.0.2.10", port: 9100 },
    },
    {
      name: "kitchen printer",
      transport: "usb",
      availability: "unavailable",
      profile: "epson-tm-t20",
      connection: {
        type: "usb",
        vendorId: 0x04b8,
        productId: 0x0202,
        bus: "001",
        address: 2,
        manufacturer: "EPSON",
        product: "TM-T20",
        serialNumber: "KITCHEN-1",
        interfaceNumber: 0,
        outEndpoints: [1],
        inEndpoints: [0x81],
      },
    },
  ],
};

function fakeSdk({
  initialSnapshot: snapshot,
  healthy = true,
  printError,
  printResult,
  stop = vi.fn(),
}: {
  initialSnapshot: Snapshot;
  healthy?: boolean | Error;
  printError?: Error;
  printResult?: Promise<{ jobId: string }>;
  stop?: ReturnType<typeof vi.fn>;
}) {
  let onSnapshot: ((next: Snapshot) => void) | undefined;
  let onError: ((error: Error) => void) | undefined;
  const printed: Array<{ printer: string; data: number[] }> = [];
  const sdk: Sdk = {
    isAvailable: vi.fn(async () => {
      if (healthy instanceof Error) throw healthy;
      return healthy;
    }),
    printers: {
      list: vi.fn(async () => snapshot),
      subscribe: vi.fn((next, options) => {
        onSnapshot = next;
        onError = options.onError;
        next(snapshot);
        return stop;
      }),
    },
    print: vi.fn(({ printer, data }) => {
      if (printError !== undefined) throw printError;
      printed.push({ printer, data: Array.from(data as Uint8Array) });
      return printResult ?? Promise.resolve({ jobId: "job-17" });
    }),
  };

  return {
    sdk,
    emit(snapshot: Snapshot) { onSnapshot?.(snapshot); },
    fail(error: Error) { onError?.(error); },
    printedJobs: () => printed,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve: (value: T) => resolve(value) };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  const main = document.createElement("main");
  main.id = "sdk-page";
  for (const [tag, id] of [
    ["p", "health-status"],
    ["p", "snapshot-status"],
    ["p", "inventory-warning"],
    ["p", "inventory-error"],
    ["select", "printer"],
    ["button", "print"],
    ["p", "print-status"],
  ] as const) {
    const element = document.createElement(tag);
    element.id = id;
    main.append(element);
  }
  document.body.replaceChildren(main);
});

test("renders the retained snapshot, exact configured names, health, and raw print result", async () => {
  // Break caught: deriving an identifier, changing the receipt bytes, or using
  // a one-shot list instead of the retained subscription makes the manual
  // page test a different path from a granted web page.
  const stop = vi.fn();
  const sdk = fakeSdk({ initialSnapshot, stop });
  const page = startSdkPage(sdk.sdk, document);
  await flush();

  expect(document.body.textContent).toContain("ESCPost is available.");
  expect(document.body.textContent).toContain(initialSnapshot.updatedAt);
  expect(document.body.textContent).toContain(initialSnapshot.warning);
  expect(page.printerOptions()).toEqual(["counter", "kitchen printer"]);

  page.selectPrinter("kitchen printer");
  await page.clickPrint();

  expect(sdk.printedJobs()).toEqual([{
    printer: "kitchen printer",
    data: [0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a],
  }]);
  expect(document.body.textContent).toContain("Print sent: job-17");

  page.unload();
  expect(stop).toHaveBeenCalledOnce();
});

test("updates the snapshot while retaining a configured selection that remains returned", async () => {
  // Break caught: rebuilding options without preserving a still-configured
  // choice silently sends a later raw job to a different printer.
  const sdk = fakeSdk({ initialSnapshot });
  const page = startSdkPage(sdk.sdk, document);
  page.selectPrinter("kitchen printer");

  sdk.emit({
    ...initialSnapshot,
    updatedAt: "2026-09-01T12:01:00Z",
    warning: null,
    printers: [initialSnapshot.printers[1]!, initialSnapshot.printers[0]!],
  });

  expect(page.selectedPrinter()).toBe("kitchen printer");
  expect(document.body.textContent).toContain("2026-09-01T12:01:00Z");
  expect(document.body.textContent).toContain("No inventory warnings.");
});

test("disables printing when the current snapshot contains no configured printers", async () => {
  // Break caught: retaining a stale printer name after it disappears lets a
  // manual page submit a raw job for a configuration no longer returned.
  const sdk = fakeSdk({ initialSnapshot });
  const page = startSdkPage(sdk.sdk, document);
  page.selectPrinter("kitchen printer");
  sdk.emit({
    updatedAt: "2026-09-01T12:02:00Z",
    warning: "No configured printers are currently returned.",
    printers: [],
  });

  expect(page.selectedPrinter()).toBe("");
  expect(page.printButton().disabled).toBe(true);
  expect(document.body.textContent).toContain("No configured printers are available.");
  await page.clickPrint();
  expect(sdk.printedJobs()).toEqual([]);
});

test("allows one pending raw job and recomputes print eligibility from later inventory", async () => {
  // Break caught: a second rapid click can submit the receipt twice, while a
  // snapshot that removes the selected printer during the job can re-enable a
  // stale raw-print action after settlement.
  const pending = deferred<{ jobId: string }>();
  const sdk = fakeSdk({ initialSnapshot, printResult: pending.promise });
  const page = startSdkPage(sdk.sdk, document);
  page.selectPrinter("kitchen printer");

  const firstClick = page.clickPrint();
  const secondClick = page.clickPrint();

  expect(sdk.printedJobs()).toEqual([{
    printer: "kitchen printer",
    data: [0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a],
  }]);
  expect(page.printButton().disabled).toBe(true);

  sdk.emit({
    updatedAt: "2026-09-01T12:03:00Z",
    warning: null,
    printers: [],
  });
  pending.resolve({ jobId: "job-18" });
  await Promise.all([firstClick, secondClick]);

  expect(page.selectedPrinter()).toBe("");
  expect(page.printButton().disabled).toBe(true);
  await page.clickPrint();
  expect(sdk.printedJobs()).toEqual([{
    printer: "kitchen printer",
    data: [0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a],
  }]);

  sdk.emit({
    ...initialSnapshot,
    updatedAt: "2026-09-01T12:04:00Z",
    printers: [initialSnapshot.printers[0]!],
  });
  expect(page.printButton().disabled).toBe(false);
  await page.clickPrint();
  expect(sdk.printedJobs()).toEqual([
    {
      printer: "kitchen printer",
      data: [0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a],
    },
    {
      printer: "counter",
      data: [0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a],
    },
  ]);
});

test("shows unavailable health, stream failures, and a rejected print without unhandled rejection", async () => {
  // Break caught: uncaught SDK errors leave the operator with a disabled or
  // misleading page and create an unhandled rejection during the manual run.
  const sdk = fakeSdk({
    initialSnapshot,
    healthy: false,
    printError: new Error("printer rejected the job"),
  });
  const page = startSdkPage(sdk.sdk, document);
  await flush();

  expect(document.body.textContent).toContain("ESCPost is unavailable.");
  sdk.fail(new Error("inventory stream disconnected"));
  expect(document.body.textContent).toContain("Inventory error: inventory stream disconnected");

  await page.clickPrint();
  expect(document.body.textContent).toContain("Print failed: printer rejected the job");
});

test("cleans up the subscription once for browser unload and repeated teardown", () => {
  // Break caught: repeated page lifecycle signals can duplicate unsubscribe
  // traffic or throw while the browser is unloading the manual page.
  const stop = vi.fn();
  const sdk = fakeSdk({ initialSnapshot, stop });
  const page = startSdkPage(sdk.sdk, document);

  testWindow.dispatchEvent(new testWindow.Event("beforeunload"));
  page.unload();

  expect(stop).toHaveBeenCalledOnce();
});
