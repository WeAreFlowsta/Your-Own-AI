/**
 * Settings > Devices: the computers this identity uses Your Own AI on, which
 * are reachable right now, and what travels between them. Taught with dots
 * and icons, not paragraphs. Changes pass between two computers that are
 * on with Flowsta Vault running, locked or not; only "Lock and stop
 * syncing" (or a Vault before 1.8.0 that is locked) holds them. One
 * sentence appears only while this is the only device: how to add another.
 */
import { $, component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { LuLaptop, LuLock, LuPlus, LuBot, LuImage, LuBrain, LuFileText, LuMessageSquare, LuLoader2 } from "@qwikest/icons/lucide";
import { Callout } from "./Callout";

interface DeviceView {
  id: string;
  name: string | null;
  platform: string | null;
  this_device: boolean;
  here: boolean | null;
  last_change_secs_ago: number | null;
  needs_update: boolean;
}

/** `line_status`: which of the other computers keep this one running the
 *  older background service, and how this computer's own move is going. */
interface LineStatus {
  old_line_wanted: boolean;
  holding: { id: string; name: string | null; app_version: string | null; needs_update: boolean }[];
  stopped_waiting: string[];
  checked_at_ms: number;
  last_pass: { at_ms: number; summary: string; skipped: string | null; failing: [string, string][] } | null;
  /** Where the running pass is; null between passes. */
  pass_progress: { kind: string; ai: number; ais: number; written: number; waiting: boolean; first_move: boolean } | null;
}

const DOWNLOAD_PAGE = "https://yourownai.net/download/";

interface DevicesView {
  /** "syncing": locked and still syncing (backups keep passing); "stopped":
   *  locked with syncing stopped; "locked": a Vault that does not say. */
  vault: "none" | "locked" | "unlocked" | "syncing" | "stopped";
  devices: DeviceView[];
  catching_up?: boolean;
}

function ago(secs: number): string {
  if (secs < 90) return "just now";
  if (secs < 5400) return `${Math.round(secs / 60)} min ago`;
  if (secs < 172800) return `${Math.round(secs / 3600)} h ago`;
  return `${Math.round(secs / 86400)} days ago`;
}

const TRAVELS: { icon: "bot" | "image" | "brain" | "file" | "chat"; label: string }[] = [
  { icon: "bot", label: "AIs" },
  { icon: "image", label: "pictures" },
  { icon: "brain", label: "memory" },
  { icon: "file", label: "documents" },
  { icon: "chat", label: "conversations" },
];

export default component$(() => {
  const view = useSignal<DevicesView | null>(null);
  const line = useSignal<LineStatus | null>(null);

  const readLine = $(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      line.value = await invoke<LineStatus>("line_status");
    } catch {
      /* an older backend: nothing to show */
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const { invoke } = await import("@tauri-apps/api/core");
    const read = async () => {
      try {
        view.value = await invoke<DevicesView>("devices_view");
      } catch {
        /* an older backend: the section stays quiet */
      }
    };
    await read();
    await readLine();
    const t = setInterval(() => {
      read();
      readLine();
    }, 30_000);
    cleanup(() => clearInterval(t));
  });

  const v = view.value;
  const others = v?.devices.filter((d) => !d.this_device) ?? [];
  // Nothing travels: syncing was stopped on this computer, or its Vault is
  // an older one that stops when locked. A locked Vault that still syncs is
  // not shown as anything: everything works.
  const locked = v?.vault === "locked" || v?.vault === "stopped";
  const lockedTip =
    v?.vault === "stopped"
      ? "Syncing is stopped on this computer. Unlock Flowsta Vault to start it again."
      : "Your Vault is locked - nothing travels to or from your other devices until it is unlocked";
  const l = line.value;
  const stopped = new Set(l?.stopped_waiting ?? []);
  const failing = l?.last_pass?.failing ?? [];
  // Reasons the move waits that the person should know (the others are
  // passing states the next pass clears).
  const skipped = l?.last_pass?.skipped ?? "";
  const paused = skipped.startsWith("the disk is almost full") || skipped.startsWith("this computer has more AIs") ? skipped : null;
  // The move itself, while it runs (later passes only keep the two versions
  // level and say nothing).
  const moving = l?.pass_progress?.first_move && l.pass_progress.ais > 0 ? l.pass_progress : null;

  return (
    <section
      id="settings-devices"
      class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
      data-testid="settings-devices"
    >
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-4">Devices</h2>

      <div class="flex flex-col gap-3">
        {/* This computer - a lock when nothing travels to or from it. */}
        <div class="flex items-center gap-3" data-testid="device-row" data-this="yes">
          <LuLaptop class="w-5 h-5 text-[var(--text-secondary)] shrink-0" />
          <span class="text-sm text-[var(--text-primary)]">This computer</span>
          {locked && (
            <span title={lockedTip} class="flex items-center" data-testid="device-locked" data-kind={v?.vault}>
              <LuLock class="w-4 h-4 text-amber-400" />
            </span>
          )}
        </div>

        {others.map((d) => {
          const name = d.name || "Another computer";
          const dot = d.needs_update ? "bg-amber-400" : d.here ? "bg-emerald-400" : d.here === false ? "bg-[var(--text-muted)] opacity-50" : "bg-[var(--text-muted)] opacity-25";
          const tip = d.needs_update
            ? `${name}: on an older version of Your Own AI`
            : d.here
              ? `${name}: on, with Flowsta Vault running - changes reach it in about a minute`
              : d.here === false
                ? `${name}: off, asleep, or its Flowsta Vault is not syncing. Changes reach it when it is back.`
                : name;
          return (
            <div key={d.id} class="flex items-center gap-3" data-testid="device-row" data-here={d.here ? "yes" : "no"} title={tip}>
              <LuLaptop class="w-5 h-5 text-[var(--text-secondary)] shrink-0" />
              <span class="text-sm text-[var(--text-primary)] truncate max-w-[220px]">{name}</span>
              <span class={`w-2.5 h-2.5 rounded-full shrink-0 ${dot}`} />
              {d.last_change_secs_ago !== null && (
                <span class="text-xs text-[var(--text-muted)]">synced {ago(d.last_change_secs_ago)}</span>
              )}
              {stopped.has(d.id) && (
                <button
                  class="text-xs text-[var(--text-muted)] underline underline-offset-2 hover:text-[var(--text-secondary)]"
                  data-testid="device-wait-again"
                  title="This computer stopped waiting for it. It waits again by itself as soon as that computer is back."
                  onClick$={async () => {
                    const { invoke } = await import("@tauri-apps/api/core");
                    await invoke("line_wait_again", { device: d.id }).catch(() => {});
                    await readLine();
                  }}
                >
                  not waiting for it: wait again
                </button>
              )}
            </div>
          );
        })}

        {/* The other computers this one keeps the older background service
            running for: an older version of Your Own AI, or one still
            moving to the new one. */}
        {(l?.holding ?? []).map((h) => {
          const name = h.name || "Another computer";
          return (
            <div key={h.id} data-testid="device-holding">
              <Callout intent="info" title={h.needs_update ? `${name} is on an older version of Your Own AI` : `${name} is still moving to the new version`}>
                This computer keeps a second background service running so the two stay in sync.
                <span class="flex flex-wrap gap-3 mt-2">
                  {h.needs_update && (
                    <button
                      class="text-xs font-medium underline underline-offset-2"
                      data-testid="device-how-to-update"
                      onClick$={async () => {
                        const { openUrl } = await import("@tauri-apps/plugin-opener");
                        await openUrl(DOWNLOAD_PAGE).catch(() => {});
                      }}
                    >
                      How to update
                    </button>
                  )}
                  <button
                    class="text-xs font-medium underline underline-offset-2"
                    data-testid="device-stop-waiting"
                    title="For a computer that is gone or no longer runs Your Own AI. Its conversations stay here. If it comes back, this computer waits for it again by itself."
                    onClick$={async () => {
                      const { invoke } = await import("@tauri-apps/api/core");
                      await invoke("line_stop_waiting", { device: h.id }).catch(() => {});
                      await readLine();
                    }}
                  >
                    Stop waiting for this computer
                  </button>
                </span>
              </Callout>
            </div>
          );
        })}

        {/* Linked, but the other computers are not known yet. */}
        {v && (v.vault === "unlocked" || v.vault === "syncing") && !v.catching_up && l?.old_line_wanted && l.checked_at_ms === 0 && (
          <div class="flex items-center gap-3" data-testid="device-waiting-to-hear">
            <LuLoader2 class="w-5 h-5 text-[var(--text-muted)] animate-spin shrink-0" />
            <span class="text-sm text-[var(--text-secondary)]">Waiting to hear from your other computers...</span>
          </div>
        )}

        {/* The move while it runs: which AI it is on, and that it waits for the person. */}
        {moving && !paused && (
          <div class="flex items-center gap-3" data-testid="device-move-running">
            <LuLoader2 class={`w-5 h-5 text-[var(--text-muted)] shrink-0 ${moving.waiting ? "" : "animate-spin"}`} />
            <span class="text-sm text-[var(--text-secondary)]">
              {`Moving your history to the new version: AI ${Math.max(moving.ai, 1)} of ${moving.ais}.`}
              {moving.waiting ? " It waits while you read or write, and carries on when you stop." : " You can keep using the app."}
            </span>
          </div>
        )}

        {/* This computer's own move to the new version, while it has not finished. */}
        {(paused || (failing.length > 0 && !moving)) && (
          <div class="flex items-center gap-3" data-testid="device-move-pending" title={paused ? undefined : failing[0]?.[1]}>
            <LuLoader2 class="w-5 h-5 text-[var(--text-muted)] shrink-0" />
            <span class="text-sm text-[var(--text-secondary)]">
              {paused
                ? `Moving your history to the new version is paused: ${paused}.`
                : `Still moving the history of ${failing.length} AI${failing.length === 1 ? "" : "s"} to the new version. It tries again every 15 minutes.`}
            </span>
          </div>
        )}

        {/* Just started or unlocked: the Vault is still reading who your
            devices are - not "this is your only one" (10-20). */}
        {v && !locked && v.catching_up && (
          <div class="flex items-center gap-3" data-testid="device-catching-up">
            <LuLoader2 class="w-5 h-5 text-[var(--text-muted)] animate-spin shrink-0" />
            <span class="text-sm text-[var(--text-secondary)]">Catching up with your other devices...</span>
          </div>
        )}

        {/* Only this computer so far: the one sentence, where it belongs. */}
        {v && !locked && !v.catching_up && others.length === 0 && (
          <div class="flex items-center gap-3" data-testid="device-add-hint">
            <span class="w-5 h-5 rounded border border-dashed border-[var(--border-subtle)] flex items-center justify-center shrink-0">
              <LuPlus class="w-3 h-3 text-[var(--text-muted)]" />
            </span>
            <span class="text-sm text-[var(--text-secondary)]">Sign in with the same Flowsta identity on another computer.</span>
          </div>
        )}
      </div>

      {/* What travels between them: one icon per kind, grows as more does. */}
      <div class="mt-5 pt-4 border-t border-[var(--border-subtle)] flex flex-wrap items-center gap-x-5 gap-y-2" data-testid="device-travels">
        {TRAVELS.map((t) => (
          <span key={t.label} class="flex items-center gap-1.5 text-xs text-[var(--text-muted)]">
            {t.icon === "bot" ? <LuBot class="w-4 h-4" /> : t.icon === "image" ? <LuImage class="w-4 h-4" /> : t.icon === "brain" ? <LuBrain class="w-4 h-4" /> : t.icon === "file" ? <LuFileText class="w-4 h-4" /> : <LuMessageSquare class="w-4 h-4" />}
            {t.label}
          </span>
        ))}
      </div>
    </section>
  );
});
