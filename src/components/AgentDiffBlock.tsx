import { component$ } from "@builder.io/qwik";
import type { DiffLine } from "../utils/lineDiff";

/**
 * The colored diff body shared by the permission card (the ask, before
 * Allow) and the work rail's completed edit step. Added lines green,
 * removed red, context muted, long unchanged stretches folded to "···".
 * The list is static once rendered - index keys are safe here.
 */
export const AgentDiffBlock = component$<{ lines: DiffLine[] }>(({ lines }) => {
  return (
    <div class="rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] overflow-hidden font-mono text-[11px] leading-relaxed">
      {lines.map((l, i) => {
        if (l.sign === "…") {
          return (
            <div key={i} class="px-3 py-0.5 text-center text-[var(--text-muted)] opacity-50 select-none">
              ···
            </div>
          );
        }
        return (
          <div
            key={i}
            class={`px-3 whitespace-pre-wrap break-all ${
              l.sign === "+"
                ? "bg-green-500/10 text-green-700 dark:text-green-300"
                : l.sign === "-"
                  ? "bg-red-500/10 text-red-700 dark:text-red-400"
                  : "text-[var(--text-muted)]"
            }`}
          >
            <span class="select-none opacity-60">{l.sign === " " ? " " : l.sign}</span>{" "}
            {l.text || " "}
          </div>
        );
      })}
    </div>
  );
});
