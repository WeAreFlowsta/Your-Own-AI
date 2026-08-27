/**
 * Add-ons > Characters - ready-made starting points for a new AI: a voice,
 * a role, a look. The shelf shows every built-in personality; "Make an AI
 * from this" opens New AI on Your AIs with that character chosen. A
 * character is a template you stamp an AI from - the AI is then yours in
 * Your AIs, and that is where it is edited.
 */

import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuSparkles, LuChevronLeft } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import { Callout } from "../../../components/Callout";
import { getArchetypeTemplates } from "../../../data/bundled-archetypes";
import type { Archetype } from "../../../types";

/** sessionStorage key Your AIs reads to open New AI with a character chosen
 *  (query params do not survive the static adapter inside Tauri). */
export const NEW_AI_FROM_CHARACTER_KEY = "newAiFromCharacter";

/** One line about the character, from its description or the opening of
 *  its personality template ("You are {{aiName}}, a warm, nurturing ... AI"). */
export function characterBlurb(a: Archetype): string {
  const d = (a.description || "").trim();
  if (d && !/^no description/i.test(d)) return d;
  const m = a.systemPromptTemplate.match(/You are \{\{aiName\}\},\s*([^.—]+)/);
  if (m) {
    const s = m[1].trim().replace(/\s+/g, " ");
    return s.charAt(0).toUpperCase() + s.slice(1) + ".";
  }
  return `${a.name} - a personality for a new AI.`;
}

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);
  const characters = getArchetypeTemplates();

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    currentModel.value = localStorage.getItem("currentModel");
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });
  const handleModelsClick = $(() => {
    nav("/setup");
  });

  const makeAi = $(async (id: string) => {
    try {
      sessionStorage.setItem(NEW_AI_FROM_CHARACTER_KEY, id);
    } catch {
      /* no session storage: Your AIs opens New AI with the default */
    }
    await nav("/your-ais");
  });

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
        <div class="max-w-5xl mx-auto px-4 py-8">
          <button
            type="button"
            onClick$={async () => {
              await nav("/add-ons");
            }}
            class="inline-flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          >
            <LuChevronLeft class="h-4 w-4" /> Add-ons
          </button>
          <h1 class="mt-2 flex items-center gap-2 text-2xl font-semibold text-[var(--text-primary)]">
            <LuSparkles class="h-6 w-6 text-[var(--text-secondary)]" /> Characters
          </h1>
          <p class="mt-1 text-[var(--text-secondary)]">
            Ready-made starting points for a new AI - a voice, a role, a look. Pick one and make it yours.
          </p>

          <Callout intent="info" title="A character is a starting point" id="characters-intro">
            Making an AI from a character copies its personality into a new AI of your own. Rename it, give it a
            portrait, knowledge and skills - the character on this shelf stays as it is for the next one.
          </Callout>

          <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {characters.map((c) => (
              <div
                key={c.id}
                class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3"
              >
                <div class="flex items-center gap-3">
                  <div class="h-14 w-14 shrink-0 overflow-hidden rounded-full border border-[var(--border-subtle)] bg-[var(--bg-main)]">
                    <img src={c.thumbnailPath} alt="" width={56} height={56} class="h-full w-full object-cover" loading="lazy" />
                  </div>
                  <div class="min-w-0">
                    <h2 class="truncate font-medium text-[var(--text-primary)]">{c.name}</h2>
                    <p class="text-xs text-[var(--text-muted)]">Made by Flowsta</p>
                  </div>
                </div>
                <p class="text-sm text-[var(--text-secondary)] line-clamp-3 flex-1">{characterBlurb(c)}</p>
                <div class="flex justify-end">
                  <LiquidMetalButton variant="secondary" onClick$={() => makeAi(c.id)} class="flex items-center h-9 px-4 sm:px-5 text-sm">
                    Make an AI from this
                  </LiquidMetalButton>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Characters - Your Own AI",
};
