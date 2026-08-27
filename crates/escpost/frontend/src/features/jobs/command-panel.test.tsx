import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen, within } from "@testing-library/preact";
import type { JobCommand } from "../../api/types";
import { CommandPanel } from "./command-panel";
import { groupAdjacentCommands } from "./model";

function character(index: number, detail: string, hex: string): JobCommand {
  return {
    byte_start: index,
    byte_end: index + 1,
    name: "Text",
    detail,
    code_bytes: "",
    capped_parameter_bytes: hex,
    total_parameter_bytes: 1,
    fixed_parameters: false,
    effects: [],
  };
}

function show(commands: JobCommand[]) {
  render(
    <CommandPanel
      groups={groupAdjacentCommands(commands, 1)}
      previewedGroupId={null}
      pinnedGroupId={null}
      previewedCharacter={null}
      onPreviewCharacter={() => {}}
      onPreviewCharacterEnd={() => {}}
      panelRef={() => {}}
      register={() => {}}
      onPreview={() => {}}
      onPreviewEnd={() => {}}
      onPin={() => {}}
    />,
  );
  const box = within(screen.getByRole("button")).getByLabelText("Parameter bytes");
  return [...box.querySelectorAll("[data-byte]")] as HTMLElement[];
}

afterEach(cleanup);

describe("a byte beside the character it printed", () => {
  test("puts the byte over the character", () => {
    const [cell] = show([character(0, "N", "4E"), character(1, "O", "4F")]);

    expect(cell.children[0].getAttribute("data-hex")).toBe("0");
    expect(cell.children[1].getAttribute("data-character")).toBe("0");
  });

  test("keeps a printed space visible instead of collapsing it", () => {
    const cells = show([character(0, "N", "4E"), character(1, " ", "20"), character(2, "O", "4F")]);
    const space = cells[1].querySelector("[data-character]");

    expect(space?.textContent).toBe(" ");
    expect(space?.className).toContain("whitespace-pre");
  });

  test("holds the line open for a byte that names no character", () => {
    const cells = show([character(0, "N", "4E"), character(1, "0xE9", "E9")]);

    // Without a character the line would collapse, which would lift the byte
    // of this cell above the bytes beside it.
    const held = cells[1].querySelector("[data-character]")?.textContent ?? "";
    expect([...held].map((letter) => letter.codePointAt(0))).toEqual([0x20]);
  });
});
