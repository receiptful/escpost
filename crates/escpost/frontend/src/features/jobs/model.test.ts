import { describe, expect, test } from "bun:test";
import type { JobCommand } from "../../api/types";
import {
  GROUP_BYTES_SHOWN,
  commandGroupView,
  groupAdjacentCommands,
  motionTerminals,
  runBoxes,
} from "./model";

function command(overrides: Partial<JobCommand> & Pick<JobCommand, "byte_start" | "byte_end" | "name">): JobCommand {
  return {
    detail: overrides.name,
    code_bytes: "",
    capped_parameter_bytes: "",
    total_parameter_bytes: 0,
    fixed_parameters: true,
    effects: [],
    ...overrides,
  };
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

  test("joins the parameter bytes of a grouped text run", () => {
    const groups = groupAdjacentCommands([
      command({
        byte_start: 0,
        byte_end: 1,
        name: "Text",
        detail: "H",
        capped_parameter_bytes: "48",
        total_parameter_bytes: 1,
      }),
      command({
        byte_start: 1,
        byte_end: 2,
        name: "Text",
        detail: "i",
        capped_parameter_bytes: "69",
        total_parameter_bytes: 1,
      }),
    ], 1);
    const view = commandGroupView(groups[0]);

    expect(view.codeBytes).toBe("");
    expect(view.parameterBytes.map((byte) => byte.hex)).toEqual(["48", "69"]);
    expect(view.totalParameterBytes).toBe(2);
  });

  test("keeps one set of bytes for a command that is not grouped", () => {
    const groups = groupAdjacentCommands([
      command({
        byte_start: 0,
        byte_end: 3,
        name: "ESC a",
        code_bytes: "1B 61",
        capped_parameter_bytes: "01",
        total_parameter_bytes: 1,
      }),
    ], 1);
    const view = commandGroupView(groups[0]);

    expect(view.codeBytes).toBe("1B 61");
    expect(view.parameterBytes.map((byte) => byte.hex)).toEqual(["01"]);
    expect(view.totalParameterBytes).toBe(1);
  });

  test("keeps every byte of a text run, so each character has one", () => {
    const letters = Array.from({ length: 15 }, (_, index) =>
      command({
        byte_start: index,
        byte_end: index + 1,
        name: "Text",
        detail: "a",
        capped_parameter_bytes: "61",
        total_parameter_bytes: 1,
      }));
    const groups = groupAdjacentCommands(letters, 1);
    const view = commandGroupView(groups[0]);

    expect(view.parameterBytes).toHaveLength(15);
    expect(view.totalParameterBytes).toBe(15);
  });

  test("stops a text run that outgrows the row, and counts the rest", () => {
    const letters = Array.from({ length: GROUP_BYTES_SHOWN + 40 }, (_, index) =>
      command({
        byte_start: index,
        byte_end: index + 1,
        name: "Text",
        detail: "a",
        capped_parameter_bytes: "61",
        total_parameter_bytes: 1,
      }));
    const groups = groupAdjacentCommands(letters, 1);
    const view = commandGroupView(groups[0]);

    expect(view.parameterBytes).toHaveLength(GROUP_BYTES_SHOWN);
    expect(view.totalParameterBytes).toBe(GROUP_BYTES_SHOWN + 40);
  });

  test("pairs bytes with characters only when each command gives one byte", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 1, name: "Text", detail: "H", capped_parameter_bytes: "48", total_parameter_bytes: 1 }),
      command({ byte_start: 1, byte_end: 2, name: "Text", detail: "i", capped_parameter_bytes: "69", total_parameter_bytes: 1 }),
      command({ byte_start: 2, byte_end: 3, name: "LF" }),
      command({ byte_start: 3, byte_end: 4, name: "LF" }),
      command({ byte_start: 4, byte_end: 7, name: "ESC a", code_bytes: "1B 61", capped_parameter_bytes: "01", total_parameter_bytes: 1 }),
    ], 1);

    expect(commandGroupView(groups[0]).characterPairing).toBe(true);
    expect(commandGroupView(groups[1]).characterPairing).toBe(false);
    expect(commandGroupView(groups[2]).characterPairing).toBe(false);
  });

  test("pairs each byte of a run with the character it printed", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 1, name: "Text", detail: "N", capped_parameter_bytes: "4E", total_parameter_bytes: 1 }),
      command({ byte_start: 1, byte_end: 2, name: "Text", detail: "O", capped_parameter_bytes: "4F", total_parameter_bytes: 1 }),
      // A byte outside the printable range has no character to name.
      command({ byte_start: 2, byte_end: 3, name: "Text", detail: "0xE9", capped_parameter_bytes: "E9", total_parameter_bytes: 1 }),
    ], 1);
    const view = commandGroupView(groups[0]);

    expect(view.parameterBytes).toEqual([
      { hex: "4E", character: "N" },
      { hex: "4F", character: "O" },
      // A byte outside the printable range has no character to name.
      { hex: "E9", character: "" },
    ]);
  });

  test("names no character for a command that prints no text", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 3, name: "ESC a", code_bytes: "1B 61", capped_parameter_bytes: "01", total_parameter_bytes: 1 }),
    ], 1);

    expect(commandGroupView(groups[0]).parameterBytes).toEqual([
      { hex: "01", character: "" },
    ]);
  });

  test("names the last byte of a command, not the byte after it", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 3, name: "ESC a" }),
      command({ byte_start: 3, byte_end: 4, name: "LF" }),
    ], 1);

    expect(commandGroupView(groups[0]).byteLast).toBe(2);
    expect(commandGroupView(groups[1]).byteLast).toBe(3);
  });

  test("treats a grouped run as having no fixed parameter size", () => {
    const groups = groupAdjacentCommands([
      command({ byte_start: 0, byte_end: 1, name: "Text", detail: "H", fixed_parameters: false }),
      command({ byte_start: 1, byte_end: 2, name: "Text", detail: "i", fixed_parameters: false }),
      command({ byte_start: 2, byte_end: 4, name: "ESC E", fixed_parameters: true }),
    ], 1);

    expect(commandGroupView(groups[0]).fixedParameters).toBe(false);
    expect(commandGroupView(groups[1]).fixedParameters).toBe(true);
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

  test("draws one box around the characters of a line", () => {
    const boxes = runBoxes([
      { x: 0, y: 0, width: 12, height: 24 },
      { x: 12, y: 0, width: 12, height: 24 },
      { x: 24, y: 0, width: 12, height: 24 },
    ]);

    expect(boxes).toEqual([{ x: 0, y: 0, width: 36, height: 24 }]);
  });

  test("starts a new box where a run continues on the next line", () => {
    const boxes = runBoxes([
      { x: 24, y: 0, width: 12, height: 24 },
      { x: 36, y: 0, width: 12, height: 24 },
      { x: 0, y: 30, width: 12, height: 24 },
      { x: 12, y: 30, width: 12, height: 24 },
    ]);

    expect(boxes).toEqual([
      { x: 24, y: 0, width: 24, height: 24 },
      { x: 0, y: 30, width: 24, height: 24 },
    ]);
  });

  test("keeps a taller character inside the box of its line", () => {
    const boxes = runBoxes([
      { x: 0, y: 12, width: 12, height: 24 },
      { x: 12, y: 12, width: 24, height: 48 },
    ]);

    expect(boxes).toEqual([{ x: 0, y: 12, width: 36, height: 48 }]);
  });

  test("knows which commands print an image", () => {
    const image = (name: string) =>
      commandGroupView(groupAdjacentCommands([
        command({ byte_start: 0, byte_end: 8, name }),
      ], 1)[0]).printsImage;

    expect(image("GS v 0")).toBe(true);
    expect(image("GS ( L")).toBe(true);
    expect(image("GS 8 L")).toBe(true);
    expect(image("ESC *")).toBe(true);
    expect(image("GS k")).toBe(false);
    expect(image("Text")).toBe(false);
  });
});
