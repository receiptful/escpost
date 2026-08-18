import type { CommandEffect, CurrentJob, JobCommand, JobSheet, Position } from "../../api/types";

export type CommandGroup = {
  id: string;
  sheetNumber: number;
  commands: JobCommand[];
};

export type GroupedSheet = JobSheet & { groups: CommandGroup[] };

export type GroupedJob = {
  sheets: GroupedSheet[];
  groups: CommandGroup[];
};

export type CommandGroupView = {
  name: string;
  byteStart: number;
  byteEnd: number;
  detail: string;
  annotation?: JobCommand["annotation"];
  paintLifecycle?: "buffered" | "committed";
  effects: CommandEffect[];
  commandCount: number;
};

export function groupAdjacentCommands(commands: JobCommand[], sheetNumber: number): CommandGroup[] {
  const groups: CommandGroup[] = [];
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
  return groups.map((group) => ({
    ...group,
    id: `sheet-${sheetNumber}:bytes-${group.commands[0].byte_start}-${group.commands.at(-1)?.byte_end}`,
  }));
}

export function groupJobCommands(job: CurrentJob): GroupedJob {
  const sheets = job.sheets.map((sheet) => ({
    ...sheet,
    groups: groupAdjacentCommands(sheet.commands, sheet.number),
  }));
  return { sheets, groups: sheets.flatMap((sheet) => sheet.groups) };
}

export function commandGroupView(group: CommandGroup): CommandGroupView {
  const first = group.commands[0];
  const last = group.commands.at(-1) ?? first;
  const text = first.name === "Text";
  const lineFeeds = first.name === "LF" && group.commands.length > 1;
  return {
    name: first.name,
    byteStart: first.byte_start,
    byteEnd: last.byte_end,
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
    commandCount: group.commands.length,
  };
}

export function effectSummary(effect: CommandEffect): string {
  if (effect.type === "state_change") {
    return `${effect.state}: ${effect.before} → ${effect.after}`;
  }
  if (effect.type === "motion") {
    return `position: (${effect.before.x}, ${effect.before.y}) → (${effect.after.x}, ${effect.after.y})`;
  }
  return `paint bounds: ${effect.bounds.width} × ${effect.bounds.height}`;
}

export function groupEffectSummary(group: CommandGroup): string {
  const view = commandGroupView(group);
  if (view.name === "Text" && view.commandCount > 1) {
    const paints = view.effects.filter((effect) => effect.type === "paint").length;
    return `${view.commandCount} text commands · ${paints} paint bounds`;
  }
  return view.effects.map(effectSummary).join(" · ") || "no visible effect";
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
