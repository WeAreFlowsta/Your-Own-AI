/**
 * Your Memory — the global page for the shared user-level profile.
 *
 * The management home for "what your AIs know about you": every AI reads
 * from this one profile, so it lives here (top-level, next to Your AIs)
 * rather than per-AI. The per-AI Memory page mirrors it read-only.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { LuArrowLeft, LuBrain } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import ProfileMemory from "../../components/ProfileMemory";
import ImportHistoryCard from "../../components/ImportHistoryCard";
import MemoryOwners from "../../components/MemoryOwners";
import { RecentlyBoard } from "../../components/memory/RecentlyBoard";
import { MemoryMapBoard } from "../../components/memory/MemoryMapBoard";
import { NotUsedList } from "../../components/memory/NotUsedList";
import { RulesBoard } from "../../components/memory/RulesBoard";
import { OpenBoard, openItems } from "../../components/memory/OpenBoard";
import { SetAsideList } from "../../components/memory/SetAsideList";

const TABS = [
  { id: "map", label: "Map" },
  { id: "recently", label: "Recently" },
  { id: "open", label: "Open" },
  { id: "rules", label: "Rules" },
] as const;
type Tab = (typeof TABS)[number]["id"];

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  /** Whose view of memory the Map shows: "you" for all the person's AIs, or one AI's id. */
  const viewAi = useSignal("you");
  const viewAgent = useSignal("");
  /** The board that is open. The address can name one: `#recently`, `#open`, `#rules`. */
  const tab = useSignal<Tab>("map");
  const openCount = useSignal(0);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const fromAddress = () => {
      const named = location.hash.replace("#", "");
      tab.value = TABS.some((t) => t.id === named) ? (named as Tab) : "map";
    };
    fromAddress();
    const count = () => void openItems().then((list) => (openCount.value = list.length));
    count();
    window.addEventListener("hashchange", fromAddress);
    window.addEventListener("yoai-memory-changed", count);
    cleanup(() => {
      window.removeEventListener("hashchange", fromAddress);
      window.removeEventListener("yoai-memory-changed", count);
    });
  });

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <AppHeader
        currentModel={null}
        handleNewQuestion$={() => nav("/chat/")}
        handleModelsClick$={() => nav("/setup/")}
        folderPath={headerWs.folderPath.value}
        folderStatus={headerWs.folderStatus.value}
        permissionMode={headerWs.permissionMode.value}
        onCloseFolder$={headerWs.closeFolder$}
        buildInstalled={headerWs.buildInstalled.value}
        recentFolders={headerWs.recentFolders.value}
        onOpenFolder$={headerWs.openFolder$}
        onBrowseFolder$={headerWs.browseFolder$}
        onOpenConversations$={headerWs.openConversations$}
      />

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-5xl mx-auto px-5 py-8">
          {/* Header */}
          <div class="flex items-center gap-3 mb-6">
            <LiquidMetalButton onClick$={() => nav("/your-ais/")} class="p-2">
              <LuArrowLeft class="w-5 h-5" />
            </LiquidMetalButton>

            <div class="w-11 h-11 rounded-full bg-gradient-to-br from-purple-500 to-indigo-600 flex items-center justify-center text-white">
              <LuBrain class="w-5 h-5" />
            </div>

            <div class="min-w-0 flex-1">
              <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela leading-tight">
                Your Memory
              </h2>
              <p class="text-sm text-[var(--text-secondary)]">
                What your AIs know about you — shared across all of them, stored
                encrypted on this device.
              </p>
            </div>
          </div>

          <p class="text-xs text-[var(--text-secondary)] mb-2">Show what can be used by</p>
          <MemoryOwners
            selected={viewAi.value}
            onAll$={() => {
              viewAi.value = "you";
            }}
            onPick$={(aiId, agentKey) => {
              viewAi.value = aiId;
              viewAgent.value = agentKey;
            }}
          />
          {viewAi.value !== "you" && (
            <p class="-mt-4 mb-6 text-xs text-[var(--text-muted)]">
              <button
                type="button"
                data-testid="open-ai-memory"
                class="underline hover:text-[var(--text-secondary)]"
                onClick$={() => {
                  sessionStorage.setItem("memory-ai-id", viewAi.value);
                  sessionStorage.setItem("memory-agent-key", viewAgent.value);
                  sessionStorage.setItem("memory-tab", "knows");
                  nav("/memory/");
                }}
              >
                Open this AI's conversations and documents
              </button>
            </p>
          )}

          {/* The four boards. */}
          <div class="flex flex-wrap gap-6 border-b border-[var(--border-subtle)] mb-6" role="tablist" data-testid="memory-tabs">
            {TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={tab.value === t.id}
                data-testid="memory-tab"
                data-tab={t.id}
                onClick$={() => {
                  tab.value = t.id;
                  history.replaceState(null, "", t.id === "map" ? location.pathname : `#${t.id}`);
                }}
                class={`-mb-px py-2 text-sm font-medium border-b-2 bg-transparent ${tab.value === t.id ? "text-[var(--text-primary)] border-[var(--bg-button-primary)]" : "text-[var(--text-secondary)] border-transparent hover:text-[var(--text-primary)]"}`}
              >
                {t.label}
                {t.id === "open" && openCount.value > 0 && (
                  <span class="ml-1.5 px-1.5 rounded-full text-[11px] bg-[var(--bg-card)] text-[var(--text-secondary)]" data-testid="memory-open-count">
                    {openCount.value}
                  </span>
                )}
              </button>
            ))}
          </div>

          {tab.value === "map" && (
            <>
              <div class="mb-8">
                <MemoryMapBoard aiId={viewAi.value} />
              </div>
              <ProfileMemory readOnly={false} rules={false} />
              <div class="mt-6">
                <SetAsideList />
              </div>
              <ImportHistoryCard />
            </>
          )}
          {tab.value === "recently" && (
            <>
              <RecentlyBoard />
              <div class="mt-6">
                <NotUsedList />
              </div>
            </>
          )}
          {tab.value === "open" && <OpenBoard />}
          {tab.value === "rules" && <RulesBoard />}

          <p class="mt-6 text-xs text-[var(--text-muted)]">
            <button type="button" data-testid="open-memory-update" onClick$={() => nav("/memory-update/")} class="underline hover:text-[var(--text-secondary)]">
              What changed in the update
            </button>
            {" · "}
            <button type="button" data-testid="open-memory-sent" onClick$={() => nav("/memory-sent/")} class="underline hover:text-[var(--text-secondary)]">
              What an online AI is sent
            </button>
          </p>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Your Memory - Your Own AI",
};
