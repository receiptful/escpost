const qz = require("@receiptful.io/escpost/qz");

if (qz.version !== "2.2.4" || typeof qz.websocket?.connect !== "function" || qz.default !== undefined) {
  throw new Error("The CommonJS QZ export must be the qz object itself.");
}
