/**
 * Memory: the one page for what the person's AIs can use. The faces choose
 * whose view the whole page shows: all the person's AIs, or one of them.
 * With an AI chosen, the Map is what that AI can use, the Rules board
 * starts with the rules that AI follows, and under the Map is what the
 * person gave that AI.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { LuArrowLeft, LuBrain } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import AiKnowledge from "../../components/AiKnowledge";
import AiKnowledgeDocuments from "../../components/AiKnowledgeDocuments";
import { useAiData } from "../../contexts/AiDataContext";
import ImportHistoryCard from "../../components/ImportHistoryCard";
import MemoryOwners from "../../components/MemoryOwners";
import { RecentlyBoard } from "../../components/memory/RecentlyBoard";
import { MemoryMapBoard } from "../../components/memory/MemoryMapBoard";
import { NotUsedList } from "../../components/memory/NotUsedList";
import { RulesBoard } from "../../components/memory/RulesBoard";
import { OpenBoard, openItems } from "../../components/memory/OpenBoard";
import { DeleteAllMemory } from "../../components/memory/DeleteAllMemory";
import { chosenProject, chooseProject, MEMORY_PROJECT_EVENT, type MemoryProject } from "../../utils/memoryProject";
import { SetAsideList } from "../../components/memory/SetAsideList";
import { RestoreNote } from "../../components/memory/RestoreNote";

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
  const aiData = useAiData();
  /** The board that is open. The address can name one: `#recently`, `#open`, `#rules`. */
  const tab = useSignal<Tab>("map");
  const openCount = useSignal(0);
  /** The project Memory is opened on, from the project's own menu. */
  const project = useSignal<MemoryProject | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const fromAddress = () => {
      const named = location.hash.replace("#", "");
      tab.value = TABS.some((t) => t.id === named) ? (named as Tab) : "map";
    };
    fromAddress();
    // An AI's card opens Memory on that AI.
    try {
      const wanted = sessionStorage.getItem("memory-view-ai");
      if (wanted) {
        sessionStorage.removeItem("memory-view-ai");
        viewAi.value = wanted;
      }
    } catch {
      /* the page opens on all the person's AIs */
    }
    const fromProject = () => (project.value = chosenProject());
    fromProject();
    window.addEventListener(MEMORY_PROJECT_EVENT, fromProject);
    const count = () => void openItems().then((list) => (openCount.value = list.length));
    count();
    window.addEventListener("hashchange", fromAddress);
    window.addEventListener("yoai-memory-changed", count);
    cleanup(() => {
      window.removeEventListener("hashchange", fromAddress);
      window.removeEventListener(MEMORY_PROJECT_EVENT, fromProject);
      window.removeEventListener("yoai-memory-changed", count);
    });
  });

  // The AI whose face is chosen, when one is.
  const picked = aiData.userDefinedAis.find((a) => a.id === viewAi.value);
  const chosen = picked ? { id: picked.id, name: picked.name } : null;

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
                Memory
              </h2>
              <p class="text-sm text-[var(--text-secondary)]">
                What your AIs can use, kept encrypted on this device.
              </p>
            </div>
          </div>

          <RestoreNote class="mb-6" />

          <MemoryOwners
            selected={viewAi.value}
            onAll$={() => {
              viewAi.value = "you";
            }}
            onPick$={(aiId) => {
              viewAi.value = aiId;
            }}
          />
          {project.value && (
            <p class="mb-4 text-sm text-[var(--text-secondary)]" data-testid="memory-project-line">
              Showing the project <span class="font-semibold text-[var(--text-primary)]">{project.value.name}</span>.{" "}
              <button type="button" data-testid="memory-project-clear" class="text-[var(--text-link)] hover:underline" onClick$={() => chooseProject(null)}>
                Show everywhere
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
                <MemoryMapBoard
                  aiId={viewAi.value}
                  project={project.value ? { id: project.value.id, name: project.value.name } : null}
                  onProject$={(id) => {
                    if (!id && project.value) chooseProject(null);
                  }}
                />
              </div>
              {chosen ? (
                // What the person gave this AI: its documents and what they wrote for it.
                <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="given-to-ai">
                  <h3 class="text-lg font-semibold text-[var(--text-primary)] mb-4">What you gave {chosen.name}</h3>
                  <AiKnowledge aiId={chosen.id} aiName={chosen.name} />
                  <div class="mt-4">
                    <AiKnowledgeDocuments aiId={chosen.id} aiName={chosen.name} />
                  </div>
                </section>
              ) : (
                <>
                  <SetAsideList />
                  <ImportHistoryCard />
                </>
              )}
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
          {tab.value === "rules" && <RulesBoard project={project.value} ai={chosen} />}

          <p class="mt-6 text-xs text-[var(--text-muted)]">
            <button type="button" data-testid="open-memory-update" onClick$={() => nav("/memory-update/")} class="underline hover:text-[var(--text-secondary)]">
              What changed in the update
            </button>
            {" · "}
            <button type="button" data-testid="open-memory-sent" onClick$={() => nav("/memory-sent/")} class="underline hover:text-[var(--text-secondary)]">
              What an online AI is sent
            </button>
          </p>
          <DeleteAllMemory />
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Your Memory - Your Own AI",
};
