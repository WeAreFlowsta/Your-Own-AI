import { loadedModelNow } from "../../utils/loadedModel";
/**
 * Add-ons - the optional things you add to your AIs. Projects (the Build
 * add-on), Skills, and the kinds that follow. Each kind lives here and is
 * where you get more of it; Your AIs is where they are used.
 */

import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuPuzzle, LuSparkles, LuWrench, LuBoxes, LuChevronRight } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import { listSkills } from "../../utils/skills";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);
  const skillCount = useSignal<number | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    loadedModelNow().then((m) => { currentModel.value = m; });
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
    skillCount.value = (await listSkills()).length;
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });
  const handleModelsClick = $(() => {
    nav("/setup");
  });

  const kinds = [
    {
      id: "skills",
      icon: LuPuzzle,
      title: "Skills",
      blurb: "What your AIs know how to do. A skill is a folder of instructions an AI reads when the work calls for it.",
      meta: skillCount.value === null ? "" : skillCount.value === 0 ? "None yet" : `${skillCount.value} installed`,
      href: "/add-ons/skills",
    },
    {
      id: "tools",
      icon: LuWrench,
      title: "Tools",
      blurb: "Programs your AIs can work in - Blender, a browser, a 3D printer, your smart home. Chosen per AI, every action approved by you.",
      meta: "In projects",
      href: "/add-ons/mcp",
    },
    {
      id: "components",
      icon: LuBoxes,
      title: "Components",
      blurb: "What this computer can get: engines for your hardware, memory, vision and OCR models, and the programs tools need. Manage them in Settings.",
      meta: "For this computer",
      href: "/add-ons/components",
    },
    {
      id: "characters",
      icon: LuSparkles,
      title: "Characters",
      blurb: "Complete AIs ready to become yours - a personality, a voice, a portrait and a starting memory. Made by Flowsta, signed, free.",
      meta: "8 characters",
      href: "/add-ons/characters",
    },
  ];

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <div class="relative z-20">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          folderPath={headerWs.folderPath.value}
          folderStatus={headerWs.folderStatus.value}
          permissionMode={headerWs.permissionMode.value}
          onCloseFolder$={headerWs.closeFolder$}
          buildInstalled={headerWs.buildInstalled.value}
          recentFolders={headerWs.recentFolders.value}
          onOpenFolder$={headerWs.openFolder$}
          onBrowseFolder$={headerWs.browseFolder$}
          onOpenConversations$={headerWs.openConversations$}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
      </div>

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-4xl mx-auto px-4 py-8">
          <h1 class="text-2xl font-semibold text-[var(--text-primary)]">Add-ons</h1>
          <p class="mt-1 text-[var(--text-secondary)]">
            The optional things you add to your AIs. Each kind lives here; your AIs are where you use them.
          </p>

          <div class="mt-6 grid gap-4 sm:grid-cols-2">
            {kinds.map((k) => {
              const Icon = k.icon;
              // Capture only the path: the card object carries an icon
              // component, which the static build cannot serialize.
              const href = k.href;
              return (
                <button
                  key={k.id}
                  type="button"
                  onClick$={async () => {
                    await nav(href);
                  }}
                  class="text-left rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-5 hover:border-[var(--text-muted)] transition-colors"
                >
                  <div class="flex items-start gap-3">
                    <Icon class="mt-0.5 h-6 w-6 shrink-0 text-[var(--text-secondary)]" />
                    <div class="min-w-0 flex-1">
                      <div class="flex items-center justify-between gap-2">
                        <h2 class="text-lg font-medium text-[var(--text-primary)]">{k.title}</h2>
                        <LuChevronRight class="h-4 w-4 shrink-0 text-[var(--text-muted)]" />
                      </div>
                      <p class="mt-1 text-sm text-[var(--text-secondary)]">{k.blurb}</p>
                      {k.meta && <p class="mt-2 text-xs text-[var(--text-muted)]">{k.meta}</p>}
                    </div>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Add-ons - Your Own AI",
};
