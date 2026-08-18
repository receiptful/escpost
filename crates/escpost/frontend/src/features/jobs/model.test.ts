import { describe, expect, test } from "bun:test";
import type { JobCommand } from "../../api/types";
import { commandGroupView, groupAdjacentCommands, motionTerminals } from "./model";

function command(overrides: Partial<JobCommand> & Pick<JobCommand, "byte_start" | "byte_end" | "name">): JobCommand {
  return { detail: overrides.name, effects: [], ...overrides };
}

describe("job visualization model", () => {
  test("groups only adjacent contiguous text and line feeds", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 1, name: "Text", detail: "H" }),
      command({ byte_start: 1, byte_end: 2, name: "Text", detail: "i" }),
      command({ byte_start: 3, byte_end: 4, name: "Text", detail: "!" }),
      command({ byte_start: 4, byte_end: 5, name: "LF", detail: "Print and line feed" }),
      command({ byte_start: 5, byte_end: 6, name: "LF", detail: "Print and line feed" }),
      command({ byte_start: 6, byte_end: 8, name: "ESC a" }),
    ], 2);

    expect(groups).toHaveLength(4);
    expect(groups.map((group) => group.id)).toEqual([
      "sheet-2:bytes-0-2",
      "sheet-2:bytes-3-4",
      "sheet-2:bytes-4-6",
      "sheet-2:bytes-6-8",
    ]);
    expect(commandGroupView(groups[0]).detail).toBe("Hi");
    expect(commandGroupView(groups[2]).detail).toBe("2x Print and line feed");
  });

  test("derives line-feed terminals from preceding paint facts", () => {
    const groups = groupAdjacentCommands([
      command({
        byte_start: 0,
        byte_end: 1,
        name: "Text",
        effects: [{ type: "paint", bounds: { x: 10, y: 0, width: 12, height: 24 } }],
      }),
      command({
        byte_start: 1,
        byte_end: 2,
        name: "LF",
        effects: [{ type: "motion", before: { x: 22, y: 12 }, after: { x: 0, y: 30 } }],
      }),
    ], 1);
    const motion = groups[1].commands[0].effects[0];
    if (motion.type !== "motion") throw new Error("expected motion");

    expect(motionTerminals(groups, 1, motion)).toEqual({
      sourceX: 22,
      sourceTop: 0,
      sourceBottom: 24,
      targetBottom: 38,
    });
  });
});
