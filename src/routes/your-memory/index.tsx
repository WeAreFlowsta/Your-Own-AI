/**
 * Your Memory — the global page for the shared user-level profile.
 *
 * The management home for "what your AIs know about you": every AI reads
 * from this one profile, so it lives here (top-level, next to Your AIs)
 * rather than per-AI. The per-AI Memory page mirrors it read-only.
 */
import { component$ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { LuArrowLeft, LuBrain } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import ProfileMemory from "../../components/ProfileMemory";
import ImportHistoryCard from "../../components/ImportHistoryCard";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <AppHeader
        currentModel={null}
        handleNewQuestion$={() => nav("/chat/")}
        handleModelsClick$={() => nav("/setup/")}
        folderPath={headerWs.folderPath.value}
        folderStatus={headerWs.folderStatus.value}
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

          <ProfileMemory readOnly={false} />

          <ImportHistoryCard />
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Your Memory - Your Own AI",
};
