import { expect, test } from "vitest";
import { EscpostError, fromSerializedError } from "../src/errors";

test("rebuilds a recognized extension failure as an EscpostError", () => {
  // Break caught: converting relay failures to plain objects loses the SDK's
  // stable error class and code that callers handle.
  const error = fromSerializedError({
    code: "PRINTER_NOT_FOUND",
    message: "No configured printer is named counter.",
  });

  expect(error).toBeInstanceOf(EscpostError);
  expect(error).toMatchObject({
    name: "EscpostError",
    code: "PRINTER_NOT_FOUND",
    message: "No configured printer is named counter.",
  });
});

test("reports malformed extension failures as a protocol mismatch", () => {
  // Break caught: accepting an unrecognized extension error code lets an
  // incompatible relay leak an undocumented error into the public SDK.
  const error = fromSerializedError({ code: "NOT_A_REAL_CODE", message: "bad" });

  expect(error).toMatchObject({
    code: "PROTOCOL_MISMATCH",
    message: "The extension returned an invalid error.",
  });
});
