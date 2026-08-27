/**
 * Add-ons > Skills - what your AIs know how to do.
 *
 * A skill is a folder of instructions (SKILL.md + supporting files, the
 * Agent Skills open standard). Installed skills live in the Build agent's
 * own skills folder, so a project session finds them on its own; in chat
 * the SKILL.md text goes to the model with the AI's instructions. Add from
 * a folder, a zip file, or a link (GitHub repository, pinned to today's
 * commit, or a direct .zip). Which AIs use a skill is set on the AI.
 */

import { component$, useSignal, useStore, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import {
  LuPuzzle,
  LuFolderPlus,
  LuPackagePlus,
  LuLink,
  LuTrash2,
  LuLoader,
  LuAlertTriangle,
  LuChevronLeft,
} from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData } from "../../../contexts/AiDataContext";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import ConfirmModal from "../../../components/ConfirmModal";
import { Callout } from "../../../components/Callout";
import {
  listSkills,
  tokensLabel,
  sourceLabel,
  usedBy,
  LARGE_SKILL_TOKENS,
  type SkillInfo,
} from "../../../utils/skills";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  const store = useStore({
    skills: [] as SkillInfo[],
    loading: true,
    // "" | "folder" | "zip" | "link" - which add path is in flight
    busy: "" as string,
    error: "",
    addOpen: false,
    link: "",
    justAdded: "" as string,
    confirmRemove: null as string | null,
    removing: false,
  });

  const load = $(async () => {
    store.loading = true;
    store.skills = await listSkills();
    store.loading = false;
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    currentModel.value = localStorage.getItem("currentModel");
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
    await load();
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });
  const handleModelsClick = $(() => {
    nav("/setup");
  });

  const finishAdd = $(async (name: string) => {
    store.justAdded = name;
    store.addOpen = false;
    store.link = "";
    await load();
  });

  const addFolder = $(async () => {
    store.error = "";
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ directory: true, multiple: false, title: "Pick the folder that holds SKILL.md" });
      if (typeof picked !== "string" || !picked) return;
      store.busy = "folder";
      const name = await invoke<string>("skills_add_folder", { path: picked });
      await finishAdd(name);
    } catch (e) {
      store.error = typeof e === "string" ? e : e instanceof Error ? e.message : "Couldn't add that skill.";
    } finally {
      store.busy = "";
    }
  });

  const addZip = $(async () => {
    store.error = "";
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        title: "Pick a skill zip file",
        filters: [{ name: "Zip", extensions: ["zip"] }],
      });
      if (typeof picked !== "string" || !picked) return;
      store.busy = "zip";
      const name = await invoke<string>("skills_add_zip", { path: picked });
      await finishAdd(name);
    } catch (e) {
      store.error = typeof e === "string" ? e : e instanceof Error ? e.message : "Couldn't add that skill.";
    } finally {
      store.busy = "";
    }
  });

  const addLink = $(async () => {
    const url = store.link.trim();
    if (!url) return;
    store.error = "";
    store.busy = "link";
    try {
      const name = await invoke<string>("skills_add_link", { url });
      await finishAdd(name);
    } catch (e) {
      store.error = typeof e === "string" ? e : e instanceof Error ? e.message : "Couldn't add that skill.";
    } finally {
      store.busy = "";
    }
  });

  const removeSkill = $(async () => {
    const name = store.confirmRemove;
    if (!name) return;
    store.removing = true;
    try {
      await invoke("skills_remove", { name });
      store.confirmRemove = null;
      await load();
    } catch (e) {
      store.error = typeof e === "string" ? e : "Couldn't remove that skill.";
    } finally {
      store.removing = false;
    }
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
        <div class="max-w-4xl mx-auto px-4 py-8">
          <button
            type="button"
            onClick$={async () => {
              await nav("/add-ons");
            }}
            class="inline-flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          >
            <LuChevronLeft class="h-4 w-4" /> Add-ons
          </button>

          <div class="mt-2 flex flex-col-reverse gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h1 class="flex items-center gap-2 text-2xl font-semibold text-[var(--text-primary)]">
                <LuPuzzle class="h-6 w-6 text-[var(--text-secondary)]" /> Skills
              </h1>
              <p class="mt-1 text-[var(--text-secondary)]">
                What your AIs know how to do. A skill is a folder of instructions an AI reads when the work calls for it.
              </p>
            </div>
            <LiquidMetalButton
              onClick$={() => {
                store.addOpen = !store.addOpen;
                store.error = "";
              }}
              class="shrink-0 flex items-center h-9 px-4 sm:px-5 text-[0.9375rem]"
            >
              Add a skill
            </LiquidMetalButton>
          </div>

          <Callout intent="info" title="How skills work" id="skills-intro">
            In a project, the AI picks a skill by its description and reads the rest as it works. In chat, the whole
            SKILL.md goes to the model with the AI's instructions, so the size shown on each card is what a chat turn
            pays to carry it. Every AI uses every skill unless you narrow it on the AI (Your AIs, Skills).
          </Callout>

          {store.addOpen && (
            <div class="mt-4 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 space-y-3">
              <p class="text-sm text-[var(--text-secondary)]">
                A skill is text - adding one never runs anything. Pick the folder or zip that holds SKILL.md, or paste a link.
              </p>
              <div class="flex flex-wrap gap-2">
                <LiquidMetalButton variant="secondary" onClick$={addFolder} disabled={!!store.busy} class="flex items-center gap-2 h-9 px-4 sm:px-5 text-[0.9375rem]">
                  {store.busy === "folder" ? <LuLoader class="h-4 w-4 animate-spin" /> : <LuFolderPlus class="h-4 w-4" />}
                  From a folder
                </LiquidMetalButton>
                <LiquidMetalButton variant="secondary" onClick$={addZip} disabled={!!store.busy} class="flex items-center gap-2 h-9 px-4 sm:px-5 text-[0.9375rem]">
                  {store.busy === "zip" ? <LuLoader class="h-4 w-4 animate-spin" /> : <LuPackagePlus class="h-4 w-4" />}
                  From a zip file
                </LiquidMetalButton>
              </div>
              <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div class="relative flex-1">
                  <LuLink class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[var(--text-muted)]" />
                  <input
                    type="url"
                    value={store.link}
                    onInput$={(_, el) => {
                      store.link = el.value;
                    }}
                    onKeyDown$={(e) => {
                      if (e.key === "Enter") addLink();
                    }}
                    placeholder="https://github.com/owner/repo  or a direct .zip link"
                    class="w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full pl-9 pr-4 py-2 text-sm placeholder-[var(--text-muted)] border border-[var(--border-subtle)] focus:outline-none focus:border-[var(--text-muted)]"
                  />
                </div>
                <LiquidMetalButton onClick$={addLink} disabled={!!store.busy || !store.link.trim()} class="flex items-center gap-2 h-9 px-4 sm:px-5 text-[0.9375rem] shrink-0">
                  {store.busy === "link" && <LuLoader class="h-4 w-4 animate-spin" />}
                  Add from link
                </LiquidMetalButton>
              </div>
              <p class="text-xs text-[var(--text-muted)]">
                A GitHub link is pinned to the commit it points at today. A link into a subfolder (…/tree/main/skills/name) works too.
              </p>
            </div>
          )}

          {store.error && (
            <div class="mt-4 flex items-start gap-2 rounded-xl border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-[var(--text-primary)]">
              <LuAlertTriangle class="mt-0.5 h-4 w-4 shrink-0 text-red-500" />
              <span>{store.error}</span>
            </div>
          )}

          <div class="mt-6">
            {store.loading ? (
              <p class="text-sm text-[var(--text-muted)]">Looking for skills…</p>
            ) : store.skills.length === 0 ? (
              <div class="rounded-2xl border border-dashed border-[var(--border-subtle)] p-8 text-center">
                <LuPuzzle class="mx-auto h-8 w-8 text-[var(--text-muted)]" />
                <p class="mt-3 text-[var(--text-primary)] font-medium">No skills yet</p>
                <p class="mt-1 text-sm text-[var(--text-secondary)]">
                  Add one from a folder, a zip file, or a link. Any skill in the open standard format works as it is.
                </p>
              </div>
            ) : (
              <div class="grid gap-4 sm:grid-cols-2">
                {store.skills.map((s) => {
                  const use = usedBy(s.name, aiData.userDefinedAis);
                  const large = s.tokens >= LARGE_SKILL_TOKENS;
                  return (
                    <div
                      key={s.name}
                      class={`rounded-2xl border bg-[var(--bg-card)] p-4 flex flex-col gap-2 ${
                        store.justAdded === s.name ? "border-[var(--text-link)]" : "border-[var(--border-subtle)]"
                      }`}
                    >
                      <div class="flex items-start justify-between gap-2">
                        <div class="min-w-0">
                          <h2 class="truncate font-medium text-[var(--text-primary)]">{s.name}</h2>
                          <p class="mt-0.5 text-sm text-[var(--text-secondary)] line-clamp-3">{s.description || "No description in SKILL.md."}</p>
                        </div>
                        <LiquidMetalButton
                          variant="danger"
                          class="p-1.5 shrink-0"
                          title="Remove this skill"
                          onClick$={() => {
                            store.confirmRemove = s.name;
                          }}
                        >
                          <LuTrash2 class="h-4 w-4" />
                        </LiquidMetalButton>
                      </div>
                      <p class="text-xs text-[var(--text-muted)]">
                        {s.files} file{s.files === 1 ? "" : "s"} · {tokensLabel(s.tokens)} ·{" "}
                        {s.runs_programs ? "can run programs" : "knowledge only"}
                      </p>
                      <p class="text-xs text-[var(--text-muted)]">{sourceLabel(s.source)}</p>
                      <p class="text-xs text-[var(--text-muted)]">
                        Used by: {use.all ? "all your AIs" : use.names.length ? use.names.join(", ") : "no AI yet"}
                      </p>
                      {large && (
                        <p class="flex items-start gap-1.5 text-xs text-amber-500">
                          <LuAlertTriangle class="mt-0.5 h-3.5 w-3.5 shrink-0" />
                          Large for a small model: in chat this takes {tokensLabel(s.tokens)} of the AI's memory every turn.
                        </p>
                      )}
                      {s.runs_programs && (
                        <p class="flex items-start gap-1.5 text-xs text-amber-500">
                          <LuAlertTriangle class="mt-0.5 h-3.5 w-3.5 shrink-0" />
                          This skill ships programs. In a project they stay blocked until you trust them.
                        </p>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>

      <ConfirmModal
        isOpen={store.confirmRemove !== null}
        title="Remove this skill?"
        message={`"${store.confirmRemove ?? ""}" will be deleted from this computer. AIs that used it simply stop using it.`}
        confirmLabel="Remove"
        variant="danger"
        busy={store.removing}
        onConfirm$={removeSkill}
        onCancel$={() => {
          store.confirmRemove = null;
        }}
      />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Skills - Your Own AI",
};
