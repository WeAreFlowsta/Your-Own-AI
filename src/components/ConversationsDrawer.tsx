import { component$, useSignal, type QRL } from "@builder.io/qwik";
import { LuFolderOpen, LuPencil, LuX } from "@qwikest/icons/lucide";
import { lastActiveAt, type ConversationListItem } from "../utils/conversationResume";

interface ConversationsDrawerProps {
  open: boolean;
  items: ConversationListItem[];
  loading: boolean;
  /** Records answered empty inside the launch grace window - "not yet", not "none". */
  warming?: boolean;
  onClose$: QRL<() => void>;
  onResume$: QRL<(item: ConversationListItem) => void>;
  onRename$: QRL<(hash: string, title: string) => void>;
}

/** Timestamps arrive in microseconds. */
function toMs(us: number): number {
  return us > 1e14 ? us / 1000 : us;
}

function timeAgo(us: number): string {
  const mins = Math.max(1, Math.round((Date.now() - toMs(us)) / 60000));
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  return `${days}d`;
}

function whenText(us: number): string {
  return new Date(toMs(us)).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * Conversations - the temporal lens on the Holochain store. Every
 * conversation is a place: click to re-enter and keep going. (The per-AI
 * Conversations tab on the Memory page is the reflective lens on the same
 * data.)
 */
export const ConversationsDrawer = component$<ConversationsDrawerProps>(
  ({ open, items, loading, warming, onClose$, onResume$, onRename$ }) => {
    const renamingHash = useSignal<string | null>(null);
    const renameDraft = useSignal("");
    /** Filter by AI - null = all. Cleared when the filtered AI has no rows. */
    const filterAiId = useSignal<string | null>(null);
    if (!open) return null;
    // The AIs that have conversations, in list order - the filter row.
    const ais: { id: string; label: string; imageUrl?: string | null }[] = [];
    for (const it of items) {
      if (!ais.some((a) => a.id === it.aiId)) {
        ais.push({ id: it.aiId, label: it.aiLabel, imageUrl: it.aiImageUrl });
      }
    }
    const activeFilter = ais.some((a) => a.id === filterAiId.value) ? filterAiId.value : null;
    const shown = activeFilter ? items.filter((i) => i.aiId === activeFilter) : items;
    return (
      <div class="fixed inset-0 z-50">
        <div class="absolute inset-0 bg-black/40" onClick$={onClose$} />
        <aside class="absolute right-0 top-0 h-full w-[340px] max-w-[85vw] bg-[var(--bg-header-footer)] border-l border-[var(--border-subtle)] shadow-2xl flex flex-col">
          <div class="flex items-center justify-between px-4 py-3 border-b border-[var(--border-subtle)]">
            <h2 class="text-base font-semibold text-[var(--text-primary)] font-varela">
              Conversations
            </h2>
            <button
              onClick$={onClose$}
              class="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              title="Close"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
          {ais.length > 1 && (
            <div class="flex items-center gap-2 px-4 py-2 border-b border-[var(--border-subtle)] overflow-x-auto">
              <button
                onClick$={() => (filterAiId.value = null)}
                title="All AIs"
                class={`shrink-0 px-2 py-0.5 rounded-full text-xs border transition-colors cursor-pointer ${
                  activeFilter === null
                    ? "border-[var(--text-secondary)] text-[var(--text-primary)] bg-[var(--bg-dropdown-hover)]"
                    : "border-[var(--border-subtle)] text-[var(--text-muted)] bg-transparent hover:text-[var(--text-primary)]"
                }`}
              >
                All
              </button>
              {ais.map((ai) => (
                <button
                  key={ai.id}
                  onClick$={() => (filterAiId.value = activeFilter === ai.id ? null : ai.id)}
                  title={activeFilter === ai.id ? `${ai.label} - show all` : `Only ${ai.label}`}
                  class={`shrink-0 rounded-full p-0.5 border-2 bg-transparent cursor-pointer transition-colors ${
                    activeFilter === ai.id
                      ? "border-[var(--text-secondary)]"
                      : "border-transparent opacity-70 hover:opacity-100"
                  }`}
                >
                  {ai.imageUrl ? (
                    <img
                      src={ai.imageUrl}
                      alt={ai.label}
                      width={28}
                      height={28}
                      class="w-7 h-7 rounded-full object-cover block"
                    />
                  ) : (
                    <span class="w-7 h-7 rounded-full bg-[var(--bg-card)] border border-[var(--border-subtle)] block" />
                  )}
                </button>
              ))}
            </div>
          )}
          <div class="flex-1 overflow-y-auto py-1">
            {loading && !warming && (
              <p class="px-4 py-3 text-sm text-[var(--text-muted)]">
                Loading your conversations..
              </p>
            )}
            {warming && items.length === 0 && (
              <div class="flex items-start gap-3 px-4 py-3 text-sm text-[var(--text-muted)]">
                <span class="mt-0.5 inline-block h-4 w-4 flex-shrink-0 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
                <span>
                  Your records are warming up - just after launch, your
                  conversations take a moment to be ready.
                </span>
              </div>
            )}
            {!loading && !warming && items.length === 0 && (
              <p class="px-4 py-3 text-sm text-[var(--text-muted)]">
                Nothing yet - your conversations will gather here, ready to
                pick back up.
              </p>
            )}
            {shown.map((item) => (
              <div
                key={item.conversation.hash}
                class="group flex w-full items-center gap-3 px-4 py-2.5 hover:bg-[var(--bg-dropdown-hover)]"
                title={`Started ${whenText(item.conversation.started_at)} · last active ${whenText(lastActiveAt(item.conversation))}`}
              >
                {item.aiImageUrl ? (
                  <img
                    src={item.aiImageUrl}
                    alt={item.aiLabel}
                    width={28}
                    height={28}
                    class="w-7 h-7 rounded-full object-cover shrink-0"
                  />
                ) : (
                  <span class="w-7 h-7 rounded-full bg-[var(--bg-card)] border border-[var(--border-subtle)] shrink-0" />
                )}
                {renamingHash.value === item.conversation.hash ? (
                  <input
                    type="text"
                    value={renameDraft.value}
                    autoFocus
                    onInput$={(_, el) => (renameDraft.value = el.value)}
                    onKeyDown$={(e) => {
                      if (e.key === "Enter") {
                        if (renameDraft.value.trim()) {
                          onRename$(item.conversation.hash, renameDraft.value.trim());
                        }
                        renamingHash.value = null;
                      } else if (e.key === "Escape") {
                        renamingHash.value = null;
                      }
                    }}
                    onBlur$={() => {
                      if (renameDraft.value.trim()) {
                        onRename$(item.conversation.hash, renameDraft.value.trim());
                      }
                      renamingHash.value = null;
                    }}
                    class="min-w-0 flex-1 bg-[var(--bg-input)] border border-[var(--border-input)] rounded-lg px-2 py-1 text-sm text-[var(--text-primary)]"
                  />
                ) : (
                  <button
                    onClick$={() => onResume$(item)}
                    class="min-w-0 flex-1 text-left bg-transparent border-none cursor-pointer"
                  >
                    <span class="block truncate text-sm text-[var(--text-primary)]">
                      {item.title}
                    </span>
                    <span class="block truncate text-xs text-[var(--text-muted)]">
                      {item.aiLabel}
                      {item.folderPath &&
                        ` · ${item.folderPath.split("/").filter(Boolean).pop()}`}
                    </span>
                  </button>
                )}
                <button
                  onClick$={() => {
                    renameDraft.value = item.title;
                    renamingHash.value = item.conversation.hash;
                  }}
                  title="Rename"
                  class="shrink-0 opacity-0 group-hover:opacity-60 hover:!opacity-100 text-[var(--text-muted)] hover:text-[var(--text-primary)] bg-transparent border-none cursor-pointer"
                >
                  <LuPencil class="h-3.5 w-3.5" />
                </button>
                {item.folderPath && (
                  <LuFolderOpen class="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]" />
                )}
                <span class="shrink-0 text-xs text-[var(--text-muted)]">
                  {timeAgo(lastActiveAt(item.conversation))}
                </span>
              </div>
            ))}
          </div>
        </aside>
      </div>
    );
  },
);
