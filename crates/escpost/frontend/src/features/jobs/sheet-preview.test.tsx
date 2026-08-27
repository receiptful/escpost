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
