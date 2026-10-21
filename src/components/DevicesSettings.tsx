/**
 * Settings > Devices: the computers this identity uses Your Own AI on, which
 * are reachable right now, and what travels between them. Taught with dots
 * and icons, not paragraphs (10-20: "it's not obvious that you need
 * both Vaults unlocked"). One sentence appears only while this is the only
 * device: how to add another.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { LuLaptop, LuLock, LuPlus, LuBot, LuImage, LuBrain, LuFileText, LuMessageSquare, LuLoader2 } from "@qwikest/icons/lucide";

interface DeviceView {
  id: string;
  name: string | null;
  platform: string | null;
  this_device: boolean;
  here: boolean | null;
  last_change_secs_ago: number | null;
  needs_update: boolean;
}

interface DevicesView {
  vault: "none" | "locked" | "unlocked";
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
    const t = setInterval(read, 30_000);
    cleanup(() => clearInterval(t));
  });

  const v = view.value;
  const others = v?.devices.filter((d) => !d.this_device) ?? [];
  const locked = v?.vault === "locked";

  return (
    <section
      id="settings-devices"
      class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
      data-testid="settings-devices"
    >
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-4">Devices</h2>

      <div class="flex flex-col gap-3">
        {/* This computer - a lock when its own Vault is locked: nothing
            travels until it is unlocked. */}
        <div class="flex items-center gap-3" data-testid="device-row" data-this="yes">
          <LuLaptop class="w-5 h-5 text-[var(--text-secondary)] shrink-0" />
          <span class="text-sm text-[var(--text-primary)]">This computer</span>
          {locked && (
            <span title="Your Vault is locked - nothing travels to or from your other devices until it is unlocked" class="flex items-center" data-testid="device-locked">
              <LuLock class="w-4 h-4 text-amber-400" />
            </span>
          )}
        </div>

        {others.map((d) => {
          const name = d.name || "Another computer";
          const dot = d.needs_update ? "bg-amber-400" : d.here ? "bg-emerald-400" : d.here === false ? "bg-[var(--text-muted)] opacity-50" : "bg-[var(--text-muted)] opacity-25";
          const tip = d.needs_update
            ? `${name}: needs the update to sync with this computer`
            : d.here
              ? `${name}: Vault unlocked and reachable - changes reach it in about a minute`
              : d.here === false
                ? `${name}: Vault locked, or the computer asleep or off - changes reach it when it is back`
                : name;
          return (
            <div key={d.id} class="flex items-center gap-3" data-testid="device-row" data-here={d.here ? "yes" : "no"} title={tip}>
              <LuLaptop class="w-5 h-5 text-[var(--text-secondary)] shrink-0" />
              <span class="text-sm text-[var(--text-primary)] truncate max-w-[220px]">{name}</span>
              <span class={`w-2.5 h-2.5 rounded-full shrink-0 ${dot}`} />
              {d.last_change_secs_ago !== null && (
                <span class="text-xs text-[var(--text-muted)]">synced {ago(d.last_change_secs_ago)}</span>
              )}
            </div>
          );
        })}

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
