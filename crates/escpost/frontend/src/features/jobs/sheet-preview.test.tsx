import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render } from "@testing-library/preact";
import type { JobCommand } from "../../api/types";
import { groupJobCommands } from "./model";
import { SheetPreview } from "./sheet-preview";

function command(overrides: Partial<JobCommand> & Pick<JobCommand, "name">): JobCommand {
  return {
    byte_start: 0,
    byte_end: 8,
    detail: overrides.name,
    code_bytes: "",
    capped_parameter_bytes: "",
    total_parameter_bytes: 0,
    fixed_parameters: true,
    effects: [{ type: "paint", bounds: { x: 0, y: 0, width: 64, height: 44 } }],
    ...overrides,
  };
}

function styled(overrides: Record<string, unknown> = {}) {
  return {
    font: "A",
    emphasized: true,
    underline_thickness: 0,
    width_magnification: 2,
    height_magnification: 2,
    reversed: false,
    justification: "center",
    code_page: 0,
    encoding: "CP437",
    international_character_set: "U.S.A.",
    right_side_character_spacing_dots: 0,
    ...overrides,
  };
}

function run(cells: { x: number; width: number }[], sheetWidth: number) {
  const commands = cells.map((cell, index) => ({
    ...command({ name: "Text", detail: "A" }),
    byte_start: index,
    byte_end: index + 1,
    capped_parameter_bytes: "41",
    total_parameter_bytes: 1,
    style: index === 0 ? styled() : undefined,
    effects: [{ type: "paint", bounds: { x: cell.x, y: 20, width: cell.width, height: 24 } }],
  }));
  const job = {
    id: "1",
    antialias: false,
    warnings: [],
    sheets: [{
      number: 1,
      name: "sheet-001.png",
      width_dots: sheetWidth,
      height_dots: 80,
      image_url: "/api/jobs/1/sheets/1",
      commands,
    }],
  };
  const grouped = groupJobCommands(job as never);
  const view = render(
    <SheetPreview
      sheet={grouped.sheets[0]}
      sheetCount={1}
      antialias={false}
      paperMargin={false}
      marginFlash={false}
      previewedGroupId={grouped.groups[0].id}
      pinnedGroupId={null}
      previewedCharacter={null}
      onPreviewCharacter={() => {}}
      onPreviewCharacterEnd={() => {}}
      register={() => {}}
      onPreview={() => {}}
      onPreviewEnd={() => {}}
      onPin={() => {}}
      onClearPin={() => {}}
    />,
  );
  return view.container.querySelector(".trace-style-label");
}

function show(commands: JobCommand[], previewed: boolean) {
  const job = {
    id: "1",
    antialias: false,
    warnings: [],
    sheets: [{
      number: 1,
      name: "sheet-001.png",
      width_dots: 384,
      height_dots: 80,
      image_url: "/api/jobs/1/sheets/1",
      commands,
    }],
  };
  const grouped = groupJobCommands(job as never);
  const view = render(
    <SheetPreview
      sheet={grouped.sheets[0]}
      sheetCount={1}
      antialias={false}
      paperMargin={false}
      marginFlash={false}
      previewedGroupId={previewed ? grouped.groups[0].id : null}
      pinnedGroupId={null}
      previewedCharacter={null}
      onPreviewCharacter={() => {}}
      onPreviewCharacterEnd={() => {}}
      register={() => {}}
      onPreview={() => {}}
      onPreviewEnd={() => {}}
      onPin={() => {}}
      onClearPin={() => {}}
    />,
  );
  return view.container.querySelector(".trace-size-label");
}

afterEach(cleanup);

describe("the size of an image", () => {
  test("names the dots an image covers while the pointer rests on it", () => {
    const label = show([command({ name: "GS v 0" })], true);

    expect(label?.textContent).toContain("64 × 44 dots");
    // The label lies below the image, over whatever follows it on the paper.
    expect(label?.getAttribute("pointer-events")).toBe("none");
  });

  test("names no size while the pointer rests elsewhere", () => {
    expect(show([command({ name: "GS v 0" })], false)).toBeNull();
  });

  test("names no size for a command that prints no image", () => {
    expect(show([command({ name: "GS k" })], true)).toBeNull();
  });
});

describe("the style of a run on the sheet", () => {
  test("names the style above the run", () => {
    const label = run([{ x: 0, width: 24 }, { x: 24, width: 24 }], 384);

    expect(label?.textContent).toContain("bold");
    expect(label?.getAttribute("pointer-events")).toBe("none");
    // Above the run, whose cells start at y 20.
    expect(Number(label?.querySelector("rect")?.getAttribute("y"))).toBeLessThan(20);
  });

  test("keeps the label on the sheet when the run ends at the right edge", () => {
    const wide = run([{ x: 300, width: 40 }, { x: 340, width: 40 }], 384);
    const box = wide?.querySelector("rect");
    const left = Number(box?.getAttribute("x"));
    const width = Number(box?.getAttribute("width"));

    expect(left).toBeGreaterThanOrEqual(0);
    expect(left + width).toBeLessThanOrEqual(384);
  });

  test("starts the label at the run when there is room to its right", () => {
    const early = run([{ x: 10, width: 24 }], 384);
    const box = early?.querySelector("rect");

    expect(Number(box?.getAttribute("x"))).toBe(10);
  });
});
