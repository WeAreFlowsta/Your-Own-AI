import {
  component$,
  useSignal,
  useVisibleTask$,
  type QRL,
} from "@builder.io/qwik";
import {
  LuTerminal,
  LuPencil,
  LuFileText,
  LuGlobe,
  LuTrash2,
  LuWrench,
  LuCheck,
  LuX,
} from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";
import type { AgentPermission } from "../types";

interface AgentPermissionCardProps {
  permission: AgentPermission;
  /** Answer with a button. `always` = the "don't ask again" checkbox. */
  onRespond$?: QRL<(decision: "allow" | "reject", always: boolean) => void>;
  /** Reports when the PENDING card scrolls out of view (drives the jump pill). */
  onOffscreenChange$?: QRL<(offscreen: boolean) => void>;
}

const DIFF_PREVIEW_LINES = 8;

function verbFor(kind?: string): string {
  switch (kind) {
    case "execute":
      return "Wants to run a command";
    case "edit":
      return "Wants to edit files";
    case "delete":
      return "Wants to delete files";
    case "move":
      return "Wants to move files";
    case "fetch":
      return "Wants to fetch from the web";
    case "read":
      return "Wants to read files";
    default:
      return "Asks for permission";
  }
}

/**
 * Inline permission card - the trust surface. Shows the EXACT thing the
 * agent asked to do (command never truncated, diff expandable), two buttons
 * plus a deliberate "don't ask again" checkbox whose wording matches the
 * agent's real persistence scope. Never a modal; collapses to a one-line
 * receipt once answered. Typing a reply while pending also answers it
 * (handled by the composer, not this card).
 */
export const AgentPermissionCard = component$<AgentPermissionCardProps>(
  ({ permission, onRespond$, onOffscreenChange$ }) => {
    const always = useSignal(false);
    const showFullDiff = useSignal(false);
    const rootRef = useSignal<HTMLDivElement>();

    // Jump-pill feed: watch our own visibility while pending.
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      track(() => permission.state);
      const el = rootRef.value;
      if (!el || permission.state !== "pending" || !onOffscreenChange$) return;
      const observer = new IntersectionObserver(
        (entries) => onOffscreenChange$(!entries[0]?.isIntersecting),
        { threshold: 0.2 },
      );
      observer.observe(el);
      cleanup(() => {
        observer.disconnect();
        onOffscreenChange$(false);
      });
    });

    const alwaysOption = permission.options.find((o) => o.kind === "allow_always");
    const alwaysLabel = alwaysOption?.name.toLowerCase().includes("session")
      ? "Don't ask again for edits this session"
      : "Don't ask again for this in this folder";

    const Icon =
      permission.kind === "execute"
        ? LuTerminal
        : permission.kind === "edit"
          ? LuPencil
          : permission.kind === "delete"
            ? LuTrash2
            : permission.kind === "fetch"
              ? LuGlobe
              : permission.kind === "read"
                ? LuFileText
                : LuWrench;

    const expired = permission.state === "expired";
    const allowed = permission.receipt?.startsWith("Allowed");
    const diffLines = permission.diff?.split("\n") ?? [];
    const diffTruncated =
      !showFullDiff.value && diffLines.length > DIFF_PREVIEW_LINES;
    const shownDiff = diffTruncated
      ? diffLines.slice(0, DIFF_PREVIEW_LINES).join("\n")
      : permission.diff;

    // Single return, ternary on state (Qwik early-return + changing state =
    // branch may never visually switch). Answered/expired = the one-line
    // receipt in place, so the transcript stays an audit log of every grant.
    return permission.state !== "pending" ? (
      <div class="max-w-4xl mx-auto w-full">
        <div class="flex items-center gap-2 px-4 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] text-sm text-[var(--text-muted)]">
          {expired ? (
            <LuX class="h-4 w-4 shrink-0 opacity-60" />
          ) : allowed ? (
            <LuCheck class="h-4 w-4 shrink-0 text-green-600 dark:text-green-400" />
          ) : (
            <LuX class="h-4 w-4 shrink-0 text-red-500 dark:text-red-400" />
          )}
          <span class="truncate">
            {expired
              ? `This request expired - the agent stopped. (${permission.command || permission.title})`
              : permission.receipt}
          </span>
        </div>
      </div>
    ) : (
      <div class="max-w-4xl mx-auto w-full" ref={rootRef}>
        <div class="rounded-xl border border-[var(--border-primary)] bg-[var(--bg-card)] p-4 space-y-3">
          <div class="flex items-center gap-2 text-sm font-medium text-[var(--text-primary)]">
            <Icon class="h-4 w-4 shrink-0" />
            {verbFor(permission.kind)}
          </div>

          {/* The exact ask - a command is NEVER truncated. */}
          {permission.command ? (
            <pre class="text-sm rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-3 whitespace-pre-wrap break-all font-mono">
              {permission.command}
            </pre>
          ) : (
            permission.title && (
              <p class="text-sm text-[var(--text-secondary)]">{permission.title}</p>
            )
          )}

          {permission.locations && permission.locations.length > 0 && (
            <div class="text-xs text-[var(--text-muted)] space-y-0.5">
              {permission.locations.map((p) => (
                <div key={p} class="font-mono truncate">
                  {p}
                </div>
              ))}
            </div>
          )}

          {permission.diff && (
            <div>
              <pre class="text-xs rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-3 whitespace-pre-wrap break-all font-mono max-h-96 overflow-y-auto">
                {shownDiff}
              </pre>
              {diffTruncated && (
                <button
                  onClick$={() => (showFullDiff.value = true)}
                  class="mt-1 text-xs text-[var(--text-link)] hover:underline"
                >
                  Show all {diffLines.length} lines
                </button>
              )}
            </div>
          )}

          {alwaysOption && (
            <label class="flex items-center gap-2 text-sm text-[var(--text-muted)] cursor-pointer select-none">
              <input
                type="checkbox"
                checked={always.value}
                onChange$={(_, el) => (always.value = el.checked)}
                class="rounded"
              />
              {alwaysLabel}
            </label>
          )}

          <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2 pt-1">
            <LiquidMetalButton
              variant="secondary"
              class="px-4 py-2 text-sm"
              onClick$={() => onRespond$?.("reject", always.value)}
            >
              Don't allow
            </LiquidMetalButton>
            <LiquidMetalButton
              class="px-4 py-2 text-sm"
              onClick$={() => onRespond$?.("allow", always.value)}
            >
              Allow
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    );
  },
);

