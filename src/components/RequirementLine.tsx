/**
 * One program a tool or component needs: detected? If not, "Install" shows
 * exactly what would run, then runs it (or opens the terminal / download
 * page). Nothing runs on the first click.
 */
import { component$, useSignal, $ } from "@builder.io/qwik";
import { LuCheck, LuLoader, LuAlertTriangle } from "@qwikest/icons/lucide";
import { requirementPlan, requirementInstall, whichProgram, type RequirementPlan } from "../utils/mcp";

export interface RequirementLineProps {
  program: string;
  label: string;
  /** Manual fallback page. */
  install: string;
  /** undefined = still checking; null = missing; string = found here */
  have: string | null | undefined;
  onChange$?: (have: string | null) => void;
}

export const RequirementLine = component$<RequirementLineProps>((props) => {
  const plan = useSignal<RequirementPlan | null>(null);
  const busy = useSignal("");
  const error = useSignal("");
  const program = props.program;
  const install = props.install;

  const act = $(async () => {
    error.value = "";
    if (!plan.value) {
      try { plan.value = await requirementPlan(program); } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
      return;
    }
    const p = plan.value;
    if (p.mode === "link") {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(p.command || install);
      return;
    }
    if (p.mode === "terminal") {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("open_in_terminal", { command: p.command, cwd: null });
      busy.value = "Running in your terminal - press Check again when it is done.";
      return;
    }
    busy.value = "Installing...";
    try {
      await requirementInstall(program);
      busy.value = "";
      plan.value = null;
      const now = await whichProgram(program);
      await props.onChange$?.(now);
    } catch (e) {
      busy.value = "";
      error.value = e instanceof Error ? e.message : String(e);
    }
  });
  const recheck = $(async () => {
    busy.value = "";
    const now = await whichProgram(program);
    await props.onChange$?.(now);
  });

  return (
    <li class="flex flex-wrap items-center gap-1.5 text-xs">
      {props.have === undefined ? (
        <LuLoader class="h-3.5 w-3.5 animate-spin text-[var(--text-muted)]" />
      ) : props.have ? (
        <LuCheck class="h-3.5 w-3.5 text-emerald-500" />
      ) : (
        <LuAlertTriangle class="h-3.5 w-3.5 text-amber-500" />
      )}
      <span class={props.have === null ? "text-amber-500" : "text-[var(--text-secondary)]"}>{props.label}</span>
      {props.have === null && (
        <span class="inline-flex flex-wrap items-center gap-2">
          <button type="button" class="text-[var(--text-link)] hover:underline disabled:opacity-60" disabled={!!busy.value} onClick$={act}>
            {busy.value ? busy.value : plan.value ? (plan.value.mode === "run" ? "Run it" : plan.value.mode === "terminal" ? "Open terminal" : "Open download page") : "Install"}
          </button>
          {busy.value && <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={recheck}>Check again</button>}
          {plan.value && !busy.value && (
            <span class="text-[var(--text-muted)]">
              {plan.value.note}{" "}
              {plan.value.mode !== "link" && <code class="rounded bg-[var(--bg-input)] px-1 py-0.5 text-[10px]">{plan.value.command}</code>}
            </span>
          )}
          {error.value && <span class="text-red-400">{error.value}</span>}
        </span>
      )}
    </li>
  );
});
