import { component$, useSignal, type QRL } from "@builder.io/qwik";
import { LuFolderOpen, LuPencil, LuX } from "@qwikest/icons/lucide";
import type { ConversationListItem } from "../utils/conversationResume";

interface ConversationsDrawerProps {
  open: boolean;
  items: ConversationListItem[];
  loading: boolean;
  onClose$: QRL<() => void>;
  onResume$: QRL<(item: ConversationListItem) => void>;
  onRename$: QRL<(hash: string, title: string) => void>;
}

function timeAgo(startedAtUs: number): string {
  // started_at arrives in microseconds.
  const ms = startedAtUs > 1e14 ? startedAtUs / 1000 : startedAtUs;
  const mins = Math.max(1, Math.round((Date.now() - ms) / 60000));
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  return `${days}d`;
}

/**
 * Conversations - the temporal lens on the Holochain store. Every
 * conversation is a place: click to re-enter and keep going. (The per-AI
 * Conversations tab on the Memory page is the reflective lens on the same
 * data.)
 */
export const ConversationsDrawer = component$<ConversationsDrawerProps>(
  ({ open, items, loading, onClose$, onResume$, onRename$ }) => {
    const renamingHash = useSignal<string | null>(null);
    const renameDraft = useSignal("");
    if (!open) return null;
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
          <div class="flex-1 overflow-y-auto py-1">
            {loading && (
              <p class="px-4 py-3 text-sm text-[var(--text-muted)]">
                Loading your conversations..
              </p>
            )}
            {!loading && items.length === 0 && (
              <p class="px-4 py-3 text-sm text-[var(--text-muted)]">
                Nothing yet - your conversations will gather here, ready to
                pick back up.
              </p>
            )}
            {items.map((item) => (
              <div
                key={item.conversation.hash}
                class="group flex w-full items-center gap-3 px-4 py-2.5 hover:bg-[var(--bg-dropdown-hover)]"
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
                  {timeAgo(item.conversation.started_at)}
                </span>
              </div>
            ))}
          </div>
        </aside>
      </div>
    );
  },
);
