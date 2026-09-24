import { component$ } from "@builder.io/qwik";
import { brandIconFor } from "../utils/brandIcons";
import { Glyph, knownGlyph } from "../utils/glyphs";
import {
  LuFileText,
  LuFolder,
  LuSearch,
  LuPencil,
  LuTrash2,
  LuArrowRightLeft,
  LuTerminal,
  LuGlobe,
  LuUsers,
  LuHourglass,
  LuSquare,
  LuPlug,
  LuSparkles,
  LuListChecks,
  LuBrain,
  LuHelpCircle,
  LuDatabase,
  LuWrench,
} from "@qwikest/icons/lucide";

/** The icon kind of a step recorded before icons existed, from its kind. */
export function iconForKind(kind: string | undefined): string {
  switch (kind) {
    case "read": return "read";
    case "list": return "folder";
    case "search": case "grep": return "search";
    case "edit": case "write": return "edit";
    case "delete": return "delete";
    case "move": return "move";
    case "execute": return "run";
    case "fetch": case "web": return "web";
    case "helper": return "helper";
    case "wait": return "wait";
    case "stop": return "stop";
    case "mcp": return "mcp";
    case "skill": return "skill";
    case "plan": return "plan";
    case "think": return "think";
    case "ask": return "ask";
    case "memory": return "memory";
    default: return "tool";
  }
}

interface ActionIconProps {
  /** actionLabels.ts IconKind; falls back from `kind` for older records. */
  icon?: string;
  kind?: string;
  status?: string;
  /** An MCP server id or a model maker: its own mark when we have one,
   *  monochrome like every other glyph. */
  brand?: string;
  /** A named glyph (a skill's own, from its front matter); wins over kind. */
  glyph?: string;
  /** Extra classes on the box. */
  class?: string;
}

/**
 * The one glyph a step carries, in a 22 px box. Always monochrome: the
 * box is a faint tint of the text, the glyph the secondary text color.
 * Color means status only - a pulsing ring while the step runs, red when
 * it failed. Same box on the rail, the permission cards and the tray.
 */
export const ActionIcon = component$<ActionIconProps>((props) => {
  const name = props.icon ?? iconForKind(props.kind);
  const running = props.status === "in_progress" || props.status === "pending";
  const failed = props.status === "failed";
  const box = `inline-flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-md ${
    failed
      ? "bg-red-500/15 text-red-500 dark:text-red-400"
      : "bg-[var(--bg-card)] text-[var(--text-secondary)]"
  } ${running ? "action-icon-running" : ""} ${props.class ?? ""}`;
  const g = "h-3.5 w-3.5";
  const brand = brandIconFor(props.brand);
  return (
    <span class={box} aria-hidden="true">
      {brand ? (
        <svg class={g} viewBox="0 0 24 24" fill="currentColor"><path d={brand} /></svg>
      ) : knownGlyph(props.glyph) ? <Glyph name={props.glyph!} class={g} />
        : name === "read" ? <LuFileText class={g} />
        : name === "folder" ? <LuFolder class={g} />
        : name === "search" ? <LuSearch class={g} />
        : name === "edit" ? <LuPencil class={g} />
        : name === "delete" ? <LuTrash2 class={g} />
        : name === "move" ? <LuArrowRightLeft class={g} />
        : name === "run" ? <LuTerminal class={g} />
        : name === "web" ? <LuGlobe class={g} />
        : name === "helper" ? <LuUsers class={g} />
        : name === "wait" ? <LuHourglass class={g} />
        : name === "stop" ? <LuSquare class={g} />
        : name === "mcp" ? <LuPlug class={g} />
        : name === "skill" ? <LuSparkles class={g} />
        : name === "plan" ? <LuListChecks class={g} />
        : name === "think" ? <LuBrain class={g} />
        : name === "ask" ? <LuHelpCircle class={g} />
        : name === "memory" ? <LuDatabase class={g} />
        : <LuWrench class={g} />}
    </span>
  );
});

/** "48 s", "4 m 12 s", "1 h 03 m" - for a step's elapsed time. */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} m ${String(s % 60).padStart(2, "0")} s`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, "0")} m`;
}
