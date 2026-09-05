const receiptBytes = new Uint8Array([0x1b, 0x40, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x0a, 0x0a, 0x0a]);

export function startSdkPage(client, pageDocument) {
  const health = requiredElement(pageDocument, "health-status");
  const snapshot = requiredElement(pageDocument, "snapshot-status");
  const warning = requiredElement(pageDocument, "inventory-warning");
  const inventoryError = requiredElement(pageDocument, "inventory-error");
  const printer = requiredElement(pageDocument, "printer");
  const printButton = requiredElement(pageDocument, "print");
  const printStatus = requiredElement(pageDocument, "print-status");
  let selectedPrinter = "";
  let stopped = false;
  let printing = false;
  let currentPrint = Promise.resolve();

  function updatePrintEligibility() {
    printButton.disabled = printing || selectedPrinter === "";
  }

  function renderSnapshot(next) {
    const previousSelection = selectedPrinter;
    printer.replaceChildren();
    for (const configuredPrinter of next.printers) {
      const option = pageDocument.createElement("option");
      option.value = configuredPrinter.name;
      option.textContent = configuredPrinter.name;
      printer.append(option);
    }
    selectedPrinter = next.printers.some((configuredPrinter) => configuredPrinter.name === previousSelection)
      ? previousSelection
      : (next.printers[0]?.name ?? "");
    printer.value = selectedPrinter;
    printer.disabled = next.printers.length === 0;
    updatePrintEligibility();
    snapshot.textContent = `Snapshot updated at: ${next.updatedAt}`;
    warning.textContent = next.warning ?? "No inventory warnings.";
    inventoryError.textContent = "";
    if (next.printers.length === 0) {
      printStatus.textContent = "No configured printers are available.";
    }
  }

  function onInventoryError(error) {
    inventoryError.textContent = `Inventory error: ${messageFor(error)}`;
  }

  async function sendRawPrint() {
    if (printing || selectedPrinter === "") return;
    printing = true;
    updatePrintEligibility();
    printStatus.textContent = "Sending raw print job…";
    try {
      const result = await client.print({ printer: selectedPrinter, data: receiptBytes });
      printStatus.textContent = `Print sent: ${result.jobId}`;
    } catch (error) {
      printStatus.textContent = `Print failed: ${messageFor(error)}`;
    } finally {
      printing = false;
      updatePrintEligibility();
    }
  }

  function unload() {
    if (stopped) return;
    stopped = true;
    pageDocument.defaultView?.removeEventListener("beforeunload", unload);
    try {
      stop();
    } catch (error) {
      inventoryError.textContent = `Inventory error: ${messageFor(error)}`;
    }
  }

  printer.addEventListener("change", () => {
    selectedPrinter = printer.value;
  });
  printButton.addEventListener("click", () => { currentPrint = sendRawPrint(); });

  let stop = () => undefined;
  try {
    stop = client.printers.subscribe(renderSnapshot, { onError: onInventoryError });
  } catch (error) {
    onInventoryError(error);
  }
  pageDocument.defaultView?.addEventListener("beforeunload", unload, { once: true });
  void client.isAvailable().then(
    (available) => { health.textContent = available ? "ESCPost is available." : "ESCPost is unavailable."; },
    () => { health.textContent = "ESCPost is unavailable."; },
  );

  return {
    printerOptions: () => Array.from(printer.options, (option) => option.value),
    selectedPrinter: () => selectedPrinter,
    selectPrinter: (name) => {
      printer.value = name;
      selectedPrinter = printer.value;
      printer.dispatchEvent(new Event("change", { bubbles: true }));
    },
    printButton: () => printButton,
    clickPrint: async () => {
      printButton.click();
      await currentPrint;
    },
    unload,
  };
}

function requiredElement(pageDocument, id) {
  const element = pageDocument.getElementById(id);
  if (element === null) throw new Error(`Missing #${id} in the SDK page.`);
  return element;
}

function messageFor(error) {
  return error instanceof Error ? error.message : String(error);
}
