import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen, within } from "@testing-library/preact";
import type { JobCommand } from "../../api/types";
import { CommandPanel } from "./command-panel";
import { groupAdjacentCommands, groupJobCommands } from "./model";

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
      byteCount={commands.length}
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

  test("shades every second byte, so one cell stands apart from the next", () => {
    const cells = show([
      character(0, "N", "4E"),
      character(1, "O", "4F"),
      character(2, "R", "52"),
    ]);

    expect(cells[0].className).toBe(cells[2].className);
    expect(cells[1].className).not.toBe(cells[0].className);
  });
});

function style(overrides: Partial<import("../../api/types").TextStyle> = {}) {
  return {
    font: "A" as const,
    emphasized: false,
    underline_thickness: 0,
    width_magnification: 1,
    height_magnification: 1,
    reversed: false,
    justification: "left" as const,
    code_page: 0,
    encoding: "CP437",
    international_character_set: "U.S.A.",
    right_side_character_spacing_dots: 0,
    ...overrides,
  };
}

function showStyled(overrides: Parameters<typeof style>[0], extra: JobCommand[] = []) {
  const job = {
    sheets: [{
      number: 1,
      commands: [
        { ...character(0, "N", "4E"), style: style(overrides) },
        character(1, "O", "4F"),
        ...extra,
      ],
    }],
  };
  const grouped = groupJobCommands(job as never);
  render(
    <CommandPanel
      groups={grouped.groups}
      byteCount={grouped.byteCount}
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
  return within(screen.getAllByRole("button")[0]).getByLabelText("Text style");
}

function styleBars() {
  return screen.queryAllByLabelText("Text style").map((bar) => ({
    alignment: within(bar).queryByLabelText("Align center") !== null,
    bold: within(bar).queryByLabelText(/^Bold/) !== null,
  }));
}

describe("the style a run printed with", () => {
  test("lists every style, and marks the ones in force", () => {
    const toolbar = showStyled({ emphasized: true, justification: "center" });
    const chip = (label: string) => within(toolbar).getByLabelText(label);

    expect(chip("Bold on").getAttribute("data-active")).toBe("true");
    expect(chip("Underline off").getAttribute("data-active")).toBe("false");
    expect(chip("Reverse off").getAttribute("data-active")).toBe("false");
  });

  test("marks one of the three alignments, and only one", () => {
    const toolbar = showStyled({ justification: "center" });

    expect(within(toolbar).getByLabelText("Align left").getAttribute("data-active")).toBe("false");
    expect(within(toolbar).getByLabelText("Align center").getAttribute("data-active")).toBe("true");
    expect(within(toolbar).getByLabelText("Align right").getAttribute("data-active")).toBe("false");
  });

  test("marks one of the two fonts, and only one", () => {
    const toolbar = showStyled({ font: "B" });

    expect(within(toolbar).getByLabelText("Font A").getAttribute("data-active")).toBe("false");
    expect(within(toolbar).getByLabelText("Font B").getAttribute("data-active")).toBe("true");
  });

  test("marks a magnification only while it magnifies", () => {
    const toolbar = showStyled({ width_magnification: 2, height_magnification: 1 });

    expect(within(toolbar).getByLabelText("Width 2x").getAttribute("data-active")).toBe("true");
    expect(within(toolbar).getByLabelText("Height 1x").getAttribute("data-active")).toBe("false");
  });
});

describe("where the style shows", () => {
  function other(name: string): JobCommand {
    return {
      byte_start: 2,
      byte_end: 4,
      name,
      detail: name,
      code_bytes: "1B 4A",
      capped_parameter_bytes: "1E",
      total_parameter_bytes: 1,
      fixed_parameters: true,
      effects: [],
    };
  }

  test("shows the style on text and on the commands that print a line", () => {
    showStyled({}, [other("LF")]);
    const bars = styleBars();

    // A line feed moves the paper by the tallest character of the line, thus
    // the font and the magnification reach it as the justification does.
    expect(bars).toHaveLength(2);
    expect(bars[0]).toEqual({ alignment: true, bold: true });
    expect(bars[1]).toEqual({ alignment: true, bold: true });
  });

  test("shows no style on a command the style does not reach", () => {
    showStyled({}, [other("GS V")]);

    expect(styleBars()).toHaveLength(1);
  });
});
