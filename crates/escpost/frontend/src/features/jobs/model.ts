import type {
  CommandEffect,
  CurrentJob,
  JobCommand,
  JobSheet,
  Position,
  TextStyle,
} from "../../api/types";

export type CommandGroup = {
  id: string;
  sheetNumber: number;
  commands: JobCommand[];
  /** The style in force while the group printed. A command that changes the
   * style carries the style it produced, thus its own group shows that one. */
  textStyle?: TextStyle;
  /** What the group shows, built once for the job rather than on each render.
   * Nothing in it follows the pointer, thus it holds while the job holds. */
  view: CommandGroupView;
};

export type GroupedSheet = JobSheet & { groups: CommandGroup[] };

export type GroupedJob = {
  sheets: GroupedSheet[];
  groups: CommandGroup[];
  /** How many bytes the job holds, which is where its last command ends. */
  byteCount: number;
};

/**
 * How many parameter bytes one row shows before the count stands for the rest.
 * Only a text run reaches this, because every other group holds one command,
 * which the server already bounds on its own.
 */
export const GROUP_BYTES_SHOWN = 256;

/** One parameter byte, beside the character it printed if it printed one. */
export type ParameterByte = { hex: string; character: string };

/**
 * The commands that put an image on the paper.
 *
 * The server names a command as the Epson command manual does, thus a name
 * stands for one command and holds while that manual holds.
 */
const IMAGE_COMMANDS = new Set(["GS v 0", "GS ( L", "GS 8 L", "ESC *"]);

/**
 * The commands that print the line the printer holds.
 *
 * The style reaches these as well: the justification places the line, and the
 * font and the height magnification decide how far the paper moves, because
 * `feed_lines` feeds by the tallest character cell of the line.
 */
const LINE_COMMANDS = new Set(["LF", "CR", "ESC J", "ESC d"]);

export type CommandGroupView = {
  name: string;
  byteStart: number;
  /** The byte after the command, for grouping and for element identity. */
  byteEnd: number;
  /** The first byte of the command, counted from one as a reader counts. */
  firstByte: number;
  /** The last byte of the command, counted from one. A range that counts from
   * one ends where a range that counts from zero ends, thus `byteEnd` serves. */
  lastByte: number;
  detail: string;
  codeBytes: string;
  /** One entry per parameter byte, so a single byte can be pointed at, each
   * beside the character it printed. */
  parameterBytes: ParameterByte[];
  totalParameterBytes: number;
  /** True when each command of the group gives exactly one byte, thus a byte
   * and a printed character stand for each other. */
  characterPairing: boolean;
  /** True when the command puts an image on the paper. */
  printsImage: boolean;
  /** True when the style of the moment reaches what this command does. */
  showsStyle: boolean;
  fixedParameters: boolean;
  annotation?: JobCommand["annotation"];
  paintLifecycle?: "buffered" | "committed";
  effects: CommandEffect[];
  /** The style in force while the group printed. */
  textStyle?: TextStyle;
};

export function groupAdjacentCommands(commands: JobCommand[], sheetNumber: number): CommandGroup[] {
  // A group holds its view once it is whole, thus the run is gathered first.
  const groups: Omit<CommandGroup, "view">[] = [];
  for (const command of commands) {
    const previous = groups.at(-1);
    const previousCommand = previous?.commands.at(-1);
    const adjacent = previousCommand?.byte_end === command.byte_start;
    const joinsText = command.name === "Text" && previousCommand?.name === "Text" && adjacent;
    const joinsLineFeeds = command.name === "LF" && previousCommand?.name === "LF" && adjacent;
    if (previous && (joinsText || joinsLineFeeds)) {
      previous.commands.push(command);
      continue;
    }
    groups.push({
      id: `sheet-${sheetNumber}:bytes-${command.byte_start}-${command.byte_end}`,
      sheetNumber,
      commands: [command],
    });
  }
  return groups.map((group) => {
    const named = {
      ...group,
      id: `sheet-${sheetNumber}:bytes-${group.commands[0].byte_start}-${group.commands.at(-1)?.byte_end}`,
    };
    return { ...named, view: commandGroupView(named) };
  });
}

export function groupJobCommands(job: CurrentJob): GroupedJob {
  const sheets = job.sheets.map((sheet) => ({
    ...sheet,
    groups: groupAdjacentCommands(sheet.commands, sheet.number),
  }));
  const groups = sheets.flatMap((sheet) => sheet.groups);
  // A command carries a style only where it changed one, thus every later
  // command prints with the last style a command carried.
  let textStyle: TextStyle | undefined;
  for (const group of groups) {
    for (const command of group.commands) {
      if (command.text_style) textStyle = command.text_style;
    }
    group.textStyle = textStyle;
    // The style arrives with the fold, thus the view is built once the group
    // knows the style it printed with.
    group.view = commandGroupView(group);
  }
  const byteCount = Math.max(
    0,
    ...groups.flatMap((group) => group.commands.map((command) => command.byte_end)),
  );
  return { sheets, groups, byteCount };
}

/** Builds what a group shows. It reads the commands and the style of the
 * group, thus it does not need the view the group ends up holding. */
export function commandGroupView(group: Omit<CommandGroup, "view">): CommandGroupView {
  const first = group.commands[0];
  const last = group.commands.at(-1) ?? first;
  const text = first.name === "Text";
  const lineFeeds = first.name === "LF" && group.commands.length > 1;
  const hexadecimal = group.commands
    .flatMap((command) => command.capped_parameter_bytes.split(" "))
    .filter((byte) => byte !== "")
    .slice(0, GROUP_BYTES_SHOWN);
  // Only a run of text pairs a byte with a character. A command such as
  // `ESC a` also carries one byte, but that byte prints nothing.
  const characterPairing = text && hexadecimal.length === group.commands.length;
  // A byte of a run printed the character its own command reports. Anything
  // else the renderer names, such as "0xE9", is not a character.
  const named = (index: number) => {
    if (!characterPairing) return "";
    const detail = group.commands[index].detail;
    return detail.length === 1 ? detail : "";
  };
  const parameterBytes = hexadecimal.map((hex, index) => ({
    hex,
    character: named(index),
  }));
  return {
    name: first.name,
    byteStart: first.byte_start,
    byteEnd: last.byte_end,
    firstByte: first.byte_start + 1,
    lastByte: last.byte_end,
    codeBytes: first.code_bytes,
    fixedParameters: group.commands.length === 1 && first.fixed_parameters,
    parameterBytes,
    characterPairing,
    printsImage: IMAGE_COMMANDS.has(first.name),
    showsStyle: text || LINE_COMMANDS.has(first.name),
    totalParameterBytes: group.commands.reduce(
      (total, command) => total + command.total_parameter_bytes,
      0,
    ),
    detail: text
      ? group.commands.map((command) => command.detail).join("")
      : lineFeeds
        ? `${group.commands.length}x ${first.detail}`
        : first.detail,
    annotation: first.annotation,
    paintLifecycle: group.commands.some((command) => command.paint_lifecycle === "buffered")
      ? "buffered"
      : group.commands.some((command) => command.paint_lifecycle === "committed")
        ? "committed"
        : undefined,
    effects: group.commands.flatMap((command) => command.effects),
    textStyle: group.textStyle,
  };
}

export type PaintBounds = Extract<CommandEffect, { type: "paint" }>["bounds"];

/**
 * Joins the cells of a run of characters into one box per printed line.
 *
 * A run holds no line feed, but the printer still wraps a line that reaches
 * the end of the print area, thus a run can cover more than one line.
 */
export function runBoxes(cells: PaintBounds[]): PaintBounds[] {
  const boxes: PaintBounds[] = [];
  let previous: PaintBounds | null = null;
  for (const cell of cells) {
    const box = boxes.at(-1);
    if (!box || !previous || cell.y !== previous.y || cell.x < previous.x) {
      boxes.push({ ...cell });
    } else {
      const right = Math.max(box.x + box.width, cell.x + cell.width);
      const bottom = Math.max(box.y + box.height, cell.y + cell.height);
      box.x = Math.min(box.x, cell.x);
      box.y = Math.min(box.y, cell.y);
      box.width = right - box.x;
      box.height = bottom - box.y;
    }
    previous = cell;
  }
  return boxes;
}

export type MotionTerminals = {
  sourceX: number;
  sourceTop: number;
  sourceBottom: number;
  targetBottom: number;
};

export function motionTerminals(
  groups: CommandGroup[],
  motionGroupIndex: number,
  motion: { before: Position; after: Position },
): MotionTerminals {
  for (let index = motionGroupIndex - 1; index >= 0; index -= 1) {
    const paints = groups[index].commands
      .flatMap((command) => command.effects)
      .filter((effect): effect is Extract<CommandEffect, { type: "paint" }> =>
        effect.type === "paint"
        && effect.bounds.y <= motion.before.y
        && effect.bounds.y + effect.bounds.height > motion.before.y
      );
    if (paints.length > 0) {
      return {
        sourceX: Math.max(...paints.map((paint) => paint.bounds.x + paint.bounds.width)),
        sourceTop: Math.min(...paints.map((paint) => paint.bounds.y)),
        sourceBottom: Math.max(...paints.map((paint) => paint.bounds.y + paint.bounds.height)),
        targetBottom: motion.after.y + 8,
      };
    }
  }
  return {
    sourceX: motion.before.x,
    sourceTop: motion.before.y,
    sourceBottom: motion.before.y,
    targetBottom: motion.after.y + 8,
  };
}
