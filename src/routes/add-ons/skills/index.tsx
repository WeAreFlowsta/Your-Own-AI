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
  LuRefreshCw,
  LuFileText,
  LuUsers,
  LuChevronDown,
} from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData, useAiDataActions } from "../../../contexts/AiDataContext";
import { renderMarkdown } from "../../../utils/renderMarkdown";
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
import { RECOMMENDED_SKILLS, SKILL_GROUPS, type RecommendedSkill } from "../../../data/recommended-skills";
import { directoryItems } from "../../../utils/directory";
import { LICENSES, currentMaker, shareSkill, shareErrorText, type ShareResult } from "../../../utils/share";
import { rememberShare, rememberedShare, fetchShareStatus, shareStatusText, type ShareStatus } from "../../../utils/shareStatus";
import { LuShare2 } from "@qwikest/icons/lucide";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();
  const { editUserAi } = useAiDataActions();
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
    // SKILL.md text per skill, once fetched; which card shows it
    preview: {} as Record<string, string>,
    previewOpen: "" as string,
    // name -> newer commit on the source branch (link installs only)
    updates: {} as Record<string, string>,
    updating: "" as string,
    // which card has its "Used by" picker open
    usedByOpen: "" as string,
    // Recommended entry being installed (by name)
    installing: "" as string,
    // The shelf: the directory's skills when it answers, the bundled list otherwise
    recommended: RECOMMENDED_SKILLS as RecommendedSkill[],
    // Share dialog: the skill being shared (folder/zip installs only - those are yours)
    shareFor: "" as string,
    shareTitle: "",
    shareDescription: "",
    shareLicense: "CC-BY-4.0",
    shareMaker: null as string | null,
    shareBusy: false,
    shareErr: "",
    shareDone: null as ShareResult | null,
    shareLicenseOpen: false,
    shareStatus: {} as Record<string, ShareStatus>,
  });

  const load = $(async () => {
    // First load shows the placeholder; a refresh after add/remove/update
    // keeps the cards in place (blanking them read as a page change).
    if (store.skills.length === 0) store.loading = true;
    store.skills = await listSkills();
    store.loading = false;
    // Where earlier shares got to - only for skills shared from this device.
    for (const sk of store.skills) {
      const r = rememberedShare("skill", sk.name);
      if (!r) continue;
      void fetchShareStatus(r).then((st) => { if (st) store.shareStatus[sk.name] = st; });
    }
    // Quiet update check for link installs - one at a time, never an error.
    const updates: Record<string, string> = {};
    for (const s of store.skills) {
      if (s.source?.kind !== "link" || !s.source.sha) continue;
      try {
        const latest = await invoke<string | null>("skills_check_update", { name: s.name });
        if (latest) updates[s.name] = latest;
      } catch {
        /* offline or rate-limited: no badge */
      }
    }
    store.updates = updates;
  });

  const togglePreview = $(async (name: string) => {
    if (store.previewOpen === name) {
      store.previewOpen = "";
      return;
    }
    if (!store.preview[name]) {
      try {
        store.preview = { ...store.preview, [name]: await invoke<string>("skills_skill_md", { name }) };
      } catch {
        store.preview = { ...store.preview, [name]: "Couldn't read SKILL.md." };
      }
    }
    store.previewOpen = name;
  });

  const updateSkill = $(async (name: string) => {
    store.error = "";
    store.updating = name;
    try {
      await invoke<string>("skills_update", { name });
      await load();
    } catch (e) {
      store.error = typeof e === "string" ? e : "Couldn't update that skill.";
    } finally {
      store.updating = "";
    }
  });

  /** Flip one AI's use of a skill. */
  const toggleAiSkill = $(async (aiId: string, skill: string) => {
    const ai = aiData.userDefinedAis.find((a) => a.id === aiId);
    if (!ai) return;
    const current = Array.isArray(ai.skills) ? ai.skills : [];
    const next = current.includes(skill) ? current.filter((n) => n !== skill) : [...current, skill];
    await editUserAi(aiId, { skills: next });
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    currentModel.value = localStorage.getItem("currentModel");
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
    // Arriving through a yourownai:// link ("Add to Your Own AI"): the Add
    // sheet opens with the link filled in. Handed over in sessionStorage.
    try {
      const link = sessionStorage.getItem("skillsAddLink");
      if (link) {
        sessionStorage.removeItem("skillsAddLink");
        store.link = link;
        store.addOpen = true;
      }
    } catch {
      /* no session storage */
    }
    await load();
    const dir = await directoryItems();
    if (dir) {
      const skills = dir.filter((d) => d.kind === "skill" && (d.source.url || d.file_url));
      if (skills.length) {
        store.recommended = skills.map((d) => ({
          name: d.id,
          title: d.name,
          group: (["Build", "Writing", "Work"].includes(d.group ?? "") ? d.group : "Work") as RecommendedSkill["group"],
          blurb: d.description,
          maker: d.maker.name,
          license: d.license,
          link: d.source.kind === "github" ? d.source.url! : d.file_url!,
          sizeChars: d.size_chars ?? 0,
        }));
      }
    }
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
    // A skill does nothing until an AI has it: open the new card's picker.
    store.usedByOpen = name;
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

  const installRecommended = $(async (name: string, link: string) => {
    store.error = "";
    store.installing = name;
    try {
      const installed = await invoke<string>("skills_add_link", { url: link });
      await finishAdd(installed);
      store.addOpen = true;
    } catch (e) {
      store.error = typeof e === "string" ? e : e instanceof Error ? e.message : "Couldn't add that skill.";
    } finally {
      store.installing = "";
    }
  });

  const openShare = $(async (s: SkillInfo) => {
    store.shareFor = s.name;
    store.shareTitle = s.name.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
    store.shareDescription = s.description;
    store.shareErr = "";
    store.shareDone = null;
    store.shareMaker = (await currentMaker())?.handle ?? null;
  });

  const doShare = $(async () => {
    const entry = store.skills.find((s) => s.name === store.shareFor);
    if (!entry) return;
    store.shareBusy = true;
    store.shareErr = "";
    try {
      const maker = await currentMaker();
      if (!maker) throw new Error("Sign in with Flowsta first - a share carries your name.");
      if (store.shareDescription.trim().length < 20) throw new Error("Say a little more about it - at least a sentence.");
      store.shareDone = await shareSkill(entry.name, {
        title: store.shareTitle.trim() || entry.name,
        description: store.shareDescription.trim(),
        license: store.shareLicense,
        runsPrograms: entry.runs_programs,
        maker,
      });
      rememberShare("skill", entry.name, store.shareDone);
      store.shareStatus[entry.name] = { state: "checking", page: store.shareDone.page, pr_url: store.shareDone.pr_url };
    } catch (e) {
      store.shareErr = shareErrorText(e);
    } finally {
      store.shareBusy = false;
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
            A skill does nothing until you give it to an AI - "Used by" on a card here, or Your AIs, edit, Skills. In
            chat the AI carries a short list of its skills and the full text of the one that fits the question, so a
            few chosen skills stay cheap; the size on each card is what that costs when it is used. In a project the
            agent sees every installed skill's description and reads one only when the work calls for it.
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
                  const users = usedBy(s.name, aiData.userDefinedAis);
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
                      {store.shareStatus[s.name] && (
                        <p class="text-xs text-[var(--text-secondary)]">
                          <span class="font-medium text-[var(--text-primary)]">Shared with everyone: </span>
                          {shareStatusText(store.shareStatus[s.name], s.name)}{" "}
                          <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={async () => {
                            const st = store.shareStatus[s.name];
                            const { openUrl } = await import("@tauri-apps/plugin-opener");
                            await openUrl(st.state === "live" ? st.page : st.pr_url);
                          }}>{store.shareStatus[s.name].state === "live" ? "Open the page" : "See the submission"}</button>
                        </p>
                      )}
                      <p class="text-xs text-[var(--text-muted)]">
                        {sourceLabel(s.source)}
                        {store.updates[s.name] && (
                          <span class="ml-2 text-amber-500">Update available ({store.updates[s.name].slice(0, 7)})</span>
                        )}
                      </p>
                      <button
                        type="button"
                        onClick$={() => {
                          store.usedByOpen = store.usedByOpen === s.name ? "" : s.name;
                        }}
                        class="inline-flex items-center gap-1.5 text-left text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
                        title="Choose which AIs use this skill"
                      >
                        <LuUsers class="h-3.5 w-3.5 shrink-0" />
                        Used by: {users.length ? users.join(", ") : "no AI yet - choose one"}
                      </button>
                      {store.usedByOpen === s.name && (
                        <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] p-3 space-y-1.5">
                          {aiData.userDefinedAis
                            .filter((a) => a.status === "active")
                            .map((a) => {
                              const on = Array.isArray(a.skills) && a.skills.includes(s.name);
                              return (
                                <label key={a.id} class="flex items-center gap-2 text-sm text-[var(--text-primary)]">
                                  <input type="checkbox" checked={on} onChange$={() => toggleAiSkill(a.id, s.name)} />
                                  {a.name}
                                </label>
                              );
                            })}
                        </div>
                      )}
                      <div class="flex flex-wrap gap-2 pt-1">
                        <LiquidMetalButton
                          variant="secondary"
                          class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                          onClick$={() => togglePreview(s.name)}
                        >
                          <LuFileText class="h-3.5 w-3.5" />
                          {store.previewOpen === s.name ? "Hide SKILL.md" : "Show SKILL.md"}
                        </LiquidMetalButton>
                        {(s.source?.kind === "folder" || s.source?.kind === "zip" || !s.source) && (
                          <LiquidMetalButton
                            variant="secondary"
                            class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                            title="List this skill for everyone, signed with your Flowsta identity"
                            onClick$={() => openShare(s)}
                          >
                            <LuShare2 class="h-3.5 w-3.5" />
                            Share
                          </LiquidMetalButton>
                        )}
                        {store.updates[s.name] && (
                          <LiquidMetalButton
                            class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                            disabled={store.updating === s.name}
                            onClick$={() => updateSkill(s.name)}
                          >
                            <LuRefreshCw class={`h-3.5 w-3.5 ${store.updating === s.name ? "animate-spin" : ""}`} />
                            Update
                          </LiquidMetalButton>
                        )}
                      </div>
                      {store.previewOpen === s.name && store.preview[s.name] && (
                        <div
                          class="markdown-content max-h-80 overflow-y-auto rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] p-3 text-sm text-[var(--text-primary)]"
                          dangerouslySetInnerHTML={renderMarkdown(store.preview[s.name])}
                        />
                      )}
                      {large && (
                        <p class="flex items-start gap-1.5 text-xs text-amber-500">
                          <LuAlertTriangle class="mt-0.5 h-3.5 w-3.5 shrink-0" />
                          Large for a small model: when it is used in chat it takes {tokensLabel(s.tokens)} of the AI's memory.
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
          <div class="mt-8">
            <h2 class="text-lg font-semibold text-[var(--text-primary)]">Ready to add</h2>
            <p class="mt-1 text-sm text-[var(--text-muted)]">
              Open-standard skills we have read and pinned - knowledge only, permissive licenses, sized for a local model. One tap adds; then choose which AIs use it.
            </p>
                {SKILL_GROUPS.filter((g) => store.recommended.some((r) => r.group === g && !store.skills.some((s) => s.name === r.name))).map((g) => (
                  <div key={g} class="mt-3">
                    <p class="text-xs uppercase tracking-wide text-[var(--text-muted)]">{g}</p>
                    <div class="mt-1.5 space-y-1.5">
                      {store.recommended.filter((r) => r.group === g && !store.skills.some((s) => s.name === r.name)).map((r) => {
                        const installed = false;
                        return (
                          <div key={r.name} class="flex items-start justify-between gap-3 rounded-xl bg-[var(--bg-main)] px-3 py-2">
                            <div class="min-w-0">
                              <p class="text-sm font-medium text-[var(--text-primary)]">{r.title}</p>
                              <p class="text-xs text-[var(--text-secondary)]">{r.blurb}</p>
                              <p class="text-xs text-[var(--text-muted)]">
                                {r.maker} · {r.license}{r.sizeChars ? ` · ~${Math.round(r.sizeChars / 4 / 100) * 100} tokens` : ""}
                              </p>
                            </div>
                            {installed ? (
                              <span class="shrink-0 text-xs text-[var(--text-muted)] pt-1">Installed</span>
                            ) : (
                              <LiquidMetalButton
                                variant="secondary"
                                class="shrink-0 flex items-center gap-1.5 px-3 py-1.5 text-xs"
                                disabled={!!store.busy || !!store.installing}
                                onClick$={() => installRecommended(r.name, r.link)}
                              >
                                {store.installing === r.name && <LuLoader class="h-3.5 w-3.5 animate-spin" />}
                                Add
                              </LiquidMetalButton>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ))}
              </div>
        </div>
      </div>

      {store.shareFor && (
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
          <div class="w-full max-w-md rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
            <h3 class="text-base font-semibold text-[var(--text-primary)]">Share "{store.shareFor}" with everyone</h3>
            {store.shareDone ? (
              <div class="mt-3 space-y-3 text-sm text-[var(--text-secondary)]">
                <p>Submitted, signed with your Flowsta identity.</p>
                <p>
                  An AI reviewer reads it against the directory rules and posts what it finds, then a person on the Your Own AI team decides. Updates to your own listing go through on their own once the checks pass.
                </p>
                <p>
                  Once it is listed it lives at{" "}
                  <span class="text-[var(--text-primary)] break-all">{store.shareDone.page}</span> - the card here shows where it stands.
                </p>
                <button
                  type="button"
                  class="text-xs text-[var(--text-link)] hover:underline"
                  onClick$={async () => {
                    const { openUrl } = await import("@tauri-apps/plugin-opener");
                    await openUrl(store.shareDone!.pr_url);
                  }}
                >
                  Open the submission
                </button>
                <LiquidMetalButton variant="secondary" class="w-full justify-center px-5 py-2 text-sm" onClick$={() => { store.shareFor = ""; }}>
                  Done
                </LiquidMetalButton>
              </div>
            ) : (
              <>
                <p class="mt-2 text-sm text-[var(--text-secondary)]">
                  Lists the skill folder on yourownai.net as a zip, signed with your Flowsta identity - yours to update or
                  remove. It is text; anyone who adds it is told if it ships programs.
                </p>
                {!store.shareMaker && (
                  <p class="mt-2 text-xs text-amber-400">Sign in with Flowsta first (Settings) - a share carries your name.</p>
                )}
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Name in the listing</label>
                <input
                  type="text"
                  value={store.shareTitle}
                  onInput$={(_, el) => { store.shareTitle = el.value; }}
                  maxLength={60}
                  class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
                />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Description</label>
                <textarea
                  value={store.shareDescription}
                  onInput$={(_, el) => { store.shareDescription = el.value; }}
                  rows={3}
                  maxLength={400}
                  class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-xl px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
                />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">License</label>
                <div class="relative mt-1">
                  <button type="button" onClick$={() => { store.shareLicenseOpen = !store.shareLicenseOpen; }} class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2 pl-4 pr-10 text-left text-sm text-[var(--text-primary)] border border-[var(--border-subtle)] focus:outline-none">
                    <span class="block truncate">{LICENSES.find((l) => l.id === store.shareLicense)?.label ?? store.shareLicense}</span>
                    <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3"><LuChevronDown class="h-4 w-4 text-[var(--text-muted)]" aria-hidden="true" /></span>
                  </button>
                  {store.shareLicenseOpen && (
                    <ul class="absolute z-10 mt-1 max-h-48 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                      {LICENSES.map((l) => (
                        <li key={l.id} class={`cursor-default select-none py-2 px-4 text-sm hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${store.shareLicense === l.id ? "bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)] font-medium" : "text-[var(--text-dropdown)]"}`} onClick$={() => { store.shareLicense = l.id; store.shareLicenseOpen = false; }}>
                          <span class="block truncate">{l.label}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
                <p class="mt-2 text-xs text-[var(--text-muted)]">Free for everyone. Paid sharing comes later, for makers who have signed their work.</p>
                <p class="mt-3 text-xs text-[var(--text-muted)]">
                  {store.shareMaker ? `Listed as @${store.shareMaker} and signed with your Flowsta identity, so people know it is yours.` : "Sign in with Flowsta first - the listing shows who made it."}
                </p>
                {store.shareErr && <p class="mt-2 text-xs text-red-400">{store.shareErr}</p>}
                <div class="mt-4 flex justify-end gap-2">
                  <LiquidMetalButton variant="secondary" onClick$={() => { store.shareFor = ""; }} disabled={store.shareBusy} class="h-9 px-5 text-sm">
                    Cancel
                  </LiquidMetalButton>
                  <LiquidMetalButton onClick$={doShare} disabled={store.shareBusy || !store.shareMaker} class="h-9 px-5 text-sm">
                    {store.shareBusy ? "Signing and sending..." : "Share"}
                  </LiquidMetalButton>
                </div>
              </>
            )}
          </div>
        </div>
      )}

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
