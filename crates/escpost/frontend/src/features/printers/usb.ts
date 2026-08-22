// How the terminal writes a USB device's identity — `{:04x}:{:04x}`, no
// `0x` — so the same printer reads the same in both interfaces, and a
// vendor:product pair copied out of one is searchable in the other.
export function usbHex(value: number) {
  return value.toString(16).padStart(4, "0");
}

// An endpoint address, on the other hand, is written the way `printers add
// --out-endpoint` takes it.
export function endpointHex(value: number) {
  return `0x${value.toString(16).padStart(2, "0")}`;
}
