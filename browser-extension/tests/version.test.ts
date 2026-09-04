import { expect, test } from "vitest";
import { extensionProtocolVersion } from "../src/protocol";

test("uses the SDK's current protocol version for extension dispatch", () => {
  // Break caught: a independently bumped relay protocol silently turns every
  // otherwise valid page request into an unavailable SDK timeout.
  expect(extensionProtocolVersion).toBe(1);
});
