import { describe, expect, it } from "vitest";
import { EscpostError, isEscpostError } from "../src/errors";

describe("EscpostError", () => {
  it("carries a typed code and a human message", () => {
    const error = new EscpostError("DAEMON_NOT_RUNNING", "escpost is not running");
    expect(error.code).toBe("DAEMON_NOT_RUNNING");
    expect(error.message).toBe("escpost is not running");
    expect(error).toBeInstanceOf(Error);
  });

  it("is recognisable across a postMessage round trip, which loses the prototype", () => {
    const plain = JSON.parse(JSON.stringify({ code: "PRINTER_NOT_FOUND", message: "no such printer" }));
    expect(isEscpostError(plain)).toBe(true);
    expect(isEscpostError(new Error("plain"))).toBe(false);
  });

  it("states that raw printing is unaffected on the codes that need it", () => {
    expect(new EscpostError("RENDER_UNAVAILABLE", "no connection").message).toContain("Raw printing is unaffected");
    expect(new EscpostError("QUOTA_EXCEEDED", "out of quota").message).toContain("Raw printing is unaffected");
  });
});
