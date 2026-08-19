import type { CommandEffect } from "../../api/types";
import { activateAnnotation, webUrl } from "./annotation";
import { commandGroupView, motionTerminals, type CommandGroup, type GroupedSheet } from "./model";

type Props = {
  sheet: GroupedSheet;
  sheetCount: number;
  antialias: boolean;
  paperMargin: boolean;
  marginFlash: boolean;
  previewedGroupId: string | null;
  pinnedGroupId: string | null;
  register: (id: string, element: SVGElement | null) => void;
  onPreview: (id: string) => void;
  onPreviewEnd: (id: string) => void;
  onPin: (id: string) => void;
  onClearPin: () => void;
};

export function SheetPreview(props: Props) {
  const { sheet } = props;
  if (!sheet.image_url || sheet.width_dots === undefined || sheet.height_dots === undefined) {
    return null;
  }
  return (
    <figure class="shrink-0 space-y-2">
      <figcaption class="flex flex-wrap items-baseline justify-between gap-2 text-sm">
        <span class="font-semibold">{sheet.name}</span>
        <span class="text-base-content/60">
          Sheet {sheet.number} of {props.sheetCount} · {sheet.width_dots} × {sheet.height_dots} printer dots
        </span>
      </figcaption>
      <div class={`receipt-paper ${props.paperMargin ? "receipt-paper-margin" : ""} ${props.marginFlash ? "receipt-paper-flash" : ""}`}>
        <div
          class="relative"
          style={{ width: `${sheet.width_dots}px`, height: `${sheet.height_dots}px` }}
          onClick={(event) => {
            if (!(event.target as Element).closest(".trace-group")) props.onClearPin();
          }}
        >
          <img
            class={`block size-full ${props.antialias ? "receipt-antialiased" : "receipt-pixelated"}`}
            src={sheet.image_url}
            alt={`Rendered receipt sheet ${sheet.number} of ${props.sheetCount}`}
            width={sheet.width_dots}
            height={sheet.height_dots}
          />
          <TraceOverlay {...props} />
        </div>
      </div>
    </figure>
  );
}

function TraceOverlay(props: Props) {
  const { sheet } = props;
  if (sheet.width_dots === undefined || sheet.height_dots === undefined) return null;
  const markerId = `trace-arrow-${sheet.number}`;
  return (
    <svg
      class="absolute inset-0 size-full overflow-visible"
      viewBox={`0 0 ${sheet.width_dots} ${sheet.height_dots}`}
      aria-label={`Trace regions for ${sheet.name}`}
    >
      <defs>
        <marker id={markerId} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="12" markerHeight="12" orient="auto" markerUnits="userSpaceOnUse">
          <path d="M 0 0 L 8 4 L 0 8 z" fill="context-stroke" />
        </marker>
      </defs>
      {sheet.groups.map((group, index) => (
        <TraceGroup
          key={group.id}
          group={group}
          groupIndex={index}
          sheetGroups={sheet.groups}
          markerId={markerId}
          previewed={props.previewedGroupId === group.id}
          pinned={props.pinnedGroupId === group.id}
          register={props.register}
          onPreview={props.onPreview}
          onPreviewEnd={props.onPreviewEnd}
          onPin={props.onPin}
        />
      ))}
    </svg>
  );
}

type TraceGroupProps = {
  group: CommandGroup;
  groupIndex: number;
  sheetGroups: CommandGroup[];
  markerId: string;
  previewed: boolean;
  pinned: boolean;
  register: Props["register"];
  onPreview: Props["onPreview"];
  onPreviewEnd: Props["onPreviewEnd"];
  onPin: Props["onPin"];
};

function TraceGroup(props: TraceGroupProps) {
  const view = commandGroupView(props.group);
  const paints = view.effects.filter(
    (effect): effect is Extract<CommandEffect, { type: "paint" }> => effect.type === "paint",
  );
  const motions = view.effects.filter(
    (effect): effect is Extract<CommandEffect, { type: "motion" }> => effect.type === "motion",
  );
  if (paints.length === 0 && motions.length === 0) return null;
  const stateClass = props.pinned ? "trace-pinned" : props.previewed ? "trace-previewed" : "";
  const activate = () => props.onPin(props.group.id);
  return (
    <g
      ref={(element) => props.register(props.group.id, element)}
      class={`trace-group ${stateClass}`}
      tabIndex={0}
      role="button"
      aria-label={`Highlight ${view.name} group at bytes ${view.byteStart} to ${view.byteEnd}`}
      aria-pressed={props.pinned}
      onPointerEnter={() => props.onPreview(props.group.id)}
      onPointerLeave={() => props.onPreviewEnd(props.group.id)}
      onFocus={() => props.onPreview(props.group.id)}
      onBlur={() => props.onPreviewEnd(props.group.id)}
      onClick={activate}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          activate();
        }
      }}
    >
      {paints.map((paint, index) => (
        <g key={`paint-${index}`}>
          <rect class="trace-region" {...paint.bounds} />
          {view.annotation && (props.previewed || props.pinned) && (
            <AnnotationLabel annotation={view.annotation} bounds={paint.bounds} />
          )}
        </g>
      ))}
      {motions.map((motion, index) => (
        <MotionDecoration key={`motion-${index}`} groups={props.sheetGroups} groupIndex={props.groupIndex} motion={motion} markerId={props.markerId} />
      ))}
    </g>
  );
}

function AnnotationLabel({ annotation, bounds }: {
  annotation: { label: string; content: string };
  bounds: Extract<CommandEffect, { type: "paint" }>["bounds"];
}) {
  const href = webUrl(annotation.content);
  const contentWidth = Math.max(28, annotation.label.length * 7 + 12);
  const labelWidth = contentWidth + 20;
  const centerX = bounds.x + bounds.width / 2;
  const labelX = centerX - labelWidth / 2;
  const labelY = bounds.y + bounds.height;
  const activate = (event: Event) => {
    event.preventDefault();
    event.stopPropagation();
    activateAnnotation(annotation.content);
  };
  return (
    <g
      class="trace-label trace-annotation-label"
      role={href ? "link" : "button"}
      aria-label={`${href ? "Copy and open" : "Copy"} QR content: ${annotation.label}`}
      tabIndex={0}
      onClick={activate}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") activate(event);
      }}
    >
      <title>{href ? "Open in browser and copy QR content" : "Copy QR content"}</title>
      <rect x={labelX} y={labelY - 9} width={labelWidth} height="18" rx="2" />
      <text x={labelX + 6} y={labelY} dy="0.35em">{annotation.label}</text>
      <text class="trace-label-action" x={labelX + labelWidth - 10} y={labelY} dy="0.35em" text-anchor="middle">{href ? "↗" : "⧉"}</text>
    </g>
  );
}

function MotionDecoration({ groups, groupIndex, motion, markerId }: {
  groups: CommandGroup[];
  groupIndex: number;
  motion: Extract<CommandEffect, { type: "motion" }>;
  markerId: string;
}) {
  const terminals = motionTerminals(groups, groupIndex, motion);
  const middleY = (terminals.sourceBottom + motion.after.y) / 2;
  const path = `M ${terminals.sourceX} ${terminals.sourceBottom} V ${middleY} H ${motion.after.x} V ${motion.after.y}`;
  const terminalPath = `M ${terminals.sourceX} ${terminals.sourceTop} V ${terminals.sourceBottom} M ${motion.after.x} ${motion.after.y} V ${terminals.targetBottom}`;
  const labelX = (motion.before.x + motion.after.x) / 2;
  return (
    <g>
      <path class="trace-motion-hit" d={`${path} ${terminalPath}`} />
      <path class="trace-motion" d={path} />
      <path class="trace-motion" d={terminalPath} markerEnd={`url(#${markerId})`} />
      <g class="trace-label">
        <rect x={labelX - 12} y={middleY - 9} width="24" height="18" rx="2" />
        <text x={labelX} y={middleY} dy="0.35em" text-anchor="middle">LF</text>
      </g>
    </g>
  );
}
