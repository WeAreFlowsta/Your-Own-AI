/**
 * What the person's AIs had learned about them before the update. It is
 * set aside: no AI uses it. Nothing here has to be done; Keep on a line
 * makes it the person's, and their AIs use it again.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import ConfirmModal from "../ConfirmModal";
import { getLocalCustomAis } from "../../utils/localAiStorage";
import { foundingOf } from "../../utils/lookalikes";

interface Aside {
  id: string;
  text: string;
  asOf: number;
  item: { state: string; source?: { for_ai?: string | null } | null };
}

const PAGE = 10;

/** "14 months ago", "3 weeks ago", "today". */
function ago(ms: number): string {
  const days = Math.max(0, Math.floor((Date.now() - ms) / 86_400_000));
  if (days < 1) return "today";
  if (days < 14) return `${days} day${days === 1 ? "" : "s"} ago`;
  if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
  const months = Math.floor(days / 30.4);
  return `${months} month${months === 1 ? "" : "s"} ago`;
}

export const SetAsideList = component$(() => {
  const rows = useSignal<Aside[]>([]);
  const names = useSignal<Record<string, string>>({});
  const shown = useSignal(PAGE);
  const busy = useSignal("");
  const asking = useSignal(false);
  const failed = useSignal("");

  const load = $(async () => {
    const all = await invoke<Aside[]>("memory_items").catch(() => []);
    rows.value = all.filter((i) => i.item.state === "set_aside").sort((a, b) => a.asOf - b.asOf);
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    const ais = await getLocalCustomAis().catch(() => []);
    const byKey: Record<string, string> = {};
    for (const a of ais) {
      byKey[a.id] = a.name;
      byKey[foundingOf(a)] = a.name;
    }
    names.value = byKey;
    await load();
  });

  const keep = $(async (id: string) => {
    busy.value = id;
    failed.value = "";
    try {
      await invoke("memory_item_change", { id, edit: { do: "keep" } });
      await load();
    } catch {
      failed.value = "Could not keep it just now.";
    } finally {
      busy.value = "";
    }
  });

  const removeAll = $(async () => {
    asking.value = false;
    busy.value = "all";
    try {
      await invoke("memory_set_aside_remove_all");
      await load();
    } catch {
      failed.value = "Could not remove them just now.";
    } finally {
      busy.value = "";
    }
  });

  if (rows.value.length === 0) return null;
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="set-aside-list">
      <h3 class="text-lg font-semibold text-[var(--text-primary)]">Learned before the update, not used</h3>
      <p class="mt-1 text-sm text-[var(--text-secondary)]">
        These are things your AIs had picked up about you before the update. They are not used now. You can keep any you want.
      </p>
      <ul class="mt-4 divide-y divide-[var(--border-subtle)]">
        {rows.value.slice(0, shown.value).map((r) => {
          const by = r.item.source?.for_ai ? names.value[r.item.source.for_ai] : "";
          return (
            <li key={r.id} class="py-3 flex items-start gap-3" data-testid="set-aside-row">
              <div class="min-w-0 flex-1">
                <div class="text-sm text-[var(--text-primary)] break-words">{r.text}</div>
                <div class="mt-0.5 text-xs text-[var(--text-muted)]">
                  Learned {ago(r.asOf)}{by ? `, by ${by}` : ""}. Not used.
                </div>
              </div>
              <button
                type="button"
                data-testid="set-aside-keep"
                disabled={busy.value !== ""}
                onClick$={() => keep(r.id)}
                class="shrink-0 text-xs font-medium text-[var(--text-link)] hover:underline disabled:opacity-50"
              >
                Keep
              </button>
            </li>
          );
        })}
      </ul>
      {failed.value && <p class="mt-2 text-xs text-red-400">{failed.value}</p>}
      <div class="mt-3 flex items-center justify-between gap-3">
        {rows.value.length > shown.value ? (
          <button type="button" data-testid="set-aside-more" onClick$={() => (shown.value += PAGE)} class="text-xs font-medium text-[var(--text-link)] hover:underline">
            Show the next {Math.min(PAGE, rows.value.length - shown.value)}
          </button>
        ) : (
          <span />
        )}
        <button type="button" data-testid="set-aside-remove-all" disabled={busy.value !== ""} onClick$={() => (asking.value = true)} class="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] hover:underline disabled:opacity-50">
          Remove all of these
        </button>
      </div>
      {asking.value && (
        <ConfirmModal
          isOpen={true}
          variant="danger"
          testId="set-aside-remove-confirm"
          title="Remove all of these?"
          message={`${rows.value.length} thing${rows.value.length === 1 ? "" : "s"} learned before the update will be removed for good. What you kept stays.`}
          confirmLabel="Remove all"
          onConfirm$={removeAll}
          onCancel$={$(() => {
            asking.value = false;
          })}
        />
      )}
    </section>
  );
});
