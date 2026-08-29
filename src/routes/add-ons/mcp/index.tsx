/**
 * Add-ons > Tools - MCP servers your AIs can work in during a project.
 *
 * A tool server is a program (Blender, a browser, a printer, a smart home)
 * that offers actions to an AI. The list is the agent's; which AIs carry a
 * server is chosen on the AI (Tools section). Presets know what a server
 * needs and fetch it only behind a button that says what and from where.
 */
import { component$, useSignal, useStore, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuWrench, LuTrash2, LuChevronLeft, LuUsers, LuAlertTriangle, LuCheck, LuLoader2 } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData, useAiDataActions } from "../../../contexts/AiDataContext";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import ConfirmModal from "../../../components/ConfirmModal";
import { Callout } from "../../../components/Callout";
import { RequirementLine } from "../../../components/RequirementLine";
import { LICENSES, currentMaker, shareTool, shareErrorText, type ShareResult } from "../../../utils/share";
import { rememberShare, rememberedShare, fetchShareStatus, shareStatusText, type ShareStatus } from "../../../utils/shareStatus";
import { LuShare2 } from "@qwikest/icons/lucide";
import {
  listMcpServers,
  addMcpServer,
  removeMcpServer,
  whichProgram,
  requirementPlan,
  requirementInstall,
  fetchGit,
  checkToolSource,
  updateToolSource,
  blenderAddonStatus,
  blenderAddonInstall,
  type BlenderAddonStatus,
  mcpUsedBy,
  mcpSummary,
  readyPresets,
  setToolConfig,
  toolConfigStatus,
  type McpPreset,
  type McpServer,
  type RequirementPlan,
} from "../../../utils/mcp";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();
  const { editUserAi } = useAiDataActions();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);
  const store = useStore({
    servers: [] as McpServer[],
    presets: [] as McpPreset[],
    loading: true,
    error: "",
    note: "",
    busy: "" as string, // preset id or "manual" while adding
    // preset readiness: program -> path | null (checked on open)
    have: {} as Record<string, string | null>,
    // "Install what it needs": the preset whose plan panel is open, its plans, progress
    installFor: "" as string,
    installPlans: [] as { program: string; label: string; install: string; plan: RequirementPlan }[],
    installStage: "" as string,
    // explicit source checks: preset id -> "checking" | "up-to-date" | "behind" | "updating" | "updated"
    sourceState: {} as Record<string, string>,
    // Blender's own add-on (only meaningful once the blender tool is added)
    blenderAddon: null as BlenderAddonStatus | null,
    blenderAddonBusy: "" as string,
    confirmRemove: "" as string,
    // settings form: which tool, the draft values, and what is filled in
    configFor: "" as string,
    configDraft: {} as Record<string, string>,
    configOk: {} as Record<string, Record<string, boolean>>,
    configSaving: false,
    // share dialog (your own tools only)
    shareFor: "" as string,
    shareTitle: "",
    shareDescription: "",
    shareLicense: "MIT",
    shareSource: "",
    shareAlso: "",
    shareBusy: false,
    shareErr: "",
    shareDone: null as ShareResult | null,
    shareMaker: null as string | null,
    shareStatus: {} as Record<string, ShareStatus>,
    usedByOpen: "" as string,
    addOpen: false,
    // manual add form
    mName: "",
    mTransport: "stdio" as "stdio" | "http",
    mCommand: "",
    mArgs: "",
    mUrl: "",
    mDescription: "",
  });

  const load = $(async () => {
    store.servers = await listMcpServers();
    store.loading = false;
    if (store.servers.some((s) => s.name === "blender")) {
      try { store.blenderAddon = await blenderAddonStatus(); } catch { /* no line */ }
    }
    for (const s of store.servers) {
      if (s.config?.length) {
        try { store.configOk[s.name] = await toolConfigStatus(s.name); } catch { /* shown as unfilled */ }
      }
      const r = rememberedShare("mcp", s.name);
      if (r) void fetchShareStatus(r).then((st) => { if (st) store.shareStatus[s.name] = st; });
    }
  });

  const openShare = $(async (s: McpServer) => {
    store.shareFor = s.name;
    store.shareTitle = s.name.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
    store.shareDescription = s.description;
    store.shareSource = "";
    store.shareAlso = "";
    store.shareErr = "";
    store.shareDone = null;
    store.shareMaker = (await currentMaker())?.handle ?? null;
  });
  const doShare = $(async () => {
    const entry = store.servers.find((s) => s.name === store.shareFor);
    if (!entry) return;
    store.shareBusy = true;
    store.shareErr = "";
    try {
      const maker = await currentMaker();
      if (!maker) throw new Error("Sign in with Flowsta first - a share carries your name.");
      if (store.shareDescription.trim().length < 20) throw new Error("Say a little more about it - at least a sentence.");
      if (!/^https:\/\//.test(store.shareSource.trim())) throw new Error("Give the tool's home page or repository (an https link) so people can see where it comes from.");
      store.shareDone = await shareTool(entry, {
        title: store.shareTitle.trim() || entry.name,
        description: store.shareDescription.trim(),
        license: store.shareLicense,
        sourceUrl: store.shareSource.trim(),
        also: store.shareAlso.trim(),
        maker,
      });
      rememberShare("mcp", entry.name, store.shareDone);
      store.shareStatus[entry.name] = { state: "checking", page: store.shareDone.page, pr_url: store.shareDone.pr_url };
    } catch (e) {
      store.shareErr = shareErrorText(e);
    } finally {
      store.shareBusy = false;
    }
  });

  const openConfig = $((name: string) => {
    const s = store.servers.find((x) => x.name === name);
    store.configFor = name;
    store.configDraft = {};
    for (const f of s?.config ?? []) store.configDraft[f.key] = f.kind === "secret" ? "" : (s?.values?.[f.key] ?? "");
  });
  const saveConfig = $(async () => {
    store.configSaving = true;
    store.error = "";
    try {
      const values: Record<string, string> = {};
      for (const [k, v] of Object.entries(store.configDraft)) if (v.trim()) values[k] = v.trim();
      store.servers = await setToolConfig(store.configFor, values);
      store.configOk[store.configFor] = await toolConfigStatus(store.configFor);
      store.configFor = "";
      store.note = "Settings saved on this computer.";
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    } finally {
      store.configSaving = false;
    }
  });
  const missingSettings = (name: string): string[] => {
    const s = store.servers.find((x) => x.name === name);
    const ok = store.configOk[name] ?? {};
    return (s?.config ?? []).filter((f) => f.required && !ok[f.key]).map((f) => f.label || f.key);
  };

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    await load();
    store.presets = await readyPresets();
    const programs = new Set<string>();
    for (const p of store.presets) for (const n of p.needs) programs.add(n.program);
    for (const prog of programs) store.have[prog] = await whichProgram(prog);
  });

  const handleNewQuestion = $(() => { nav("/chat"); });
  const handleModelsClick = $(() => { nav("/setup"); });

  // One button for everything a tool needs: show every step first (what
  // runs, from where), then run them in order; things that need a password
  // open the terminal; when all are present, carry on to the add itself.
  const openInstallAll = $(async (id: string) => {
    const preset = store.presets.find((p) => p.id === id);
    if (!preset) return;
    store.error = "";
    store.installStage = "";
    const missing = preset.needs.filter((n) => store.have[n.program] === null);
    const plans = [];
    for (const n of missing) {
      try { plans.push({ ...n, plan: await requirementPlan(n.program) }); }
      catch (e) { store.error = e instanceof Error ? e.message : String(e); return; }
    }
    store.installPlans = plans;
    store.installFor = id;
  });
  const runInstallAll = $(async () => {
    const id = store.installFor;
    const preset = store.presets.find((p) => p.id === id);
    if (!preset) return;
    store.busy = id;
    let needsTerminal = false;
    try {
      for (const item of store.installPlans) {
        if (item.plan.mode === "run") {
          store.installStage = `Installing ${item.program}...`;
          await requirementInstall(item.program);
        } else if (item.plan.mode === "terminal") {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("open_in_terminal", { command: item.plan.command, cwd: null });
          needsTerminal = true;
        } else {
          const { openUrl } = await import("@tauri-apps/plugin-opener");
          await openUrl(item.plan.command || item.install);
          needsTerminal = true;
        }
      }
      for (const n of preset.needs) store.have[n.program] = await whichProgram(n.program);
      const stillMissing = preset.needs.filter((n) => store.have[n.program] === null);
      if (stillMissing.length) {
        store.installStage = needsTerminal
          ? `Finish ${stillMissing.map((n) => n.program).join(", ")} in your terminal, then press Check again.`
          : `${stillMissing.map((n) => n.program).join(", ")} still not found - open a new terminal or restart the app, then Check again.`;
        return;
      }
      store.installFor = "";
      store.installStage = "";
      store.busy = "";
      await addPreset(id);
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    } finally {
      if (store.busy === id) store.busy = "";
    }
  });
  const checkSource = $(async (id: string) => {
    store.error = "";
    store.sourceState[id] = "checking";
    try {
      const st = await checkToolSource(id);
      store.sourceState[id] = st.behind ? "behind" : "up-to-date";
    } catch (e) {
      store.sourceState[id] = "";
      store.error = e instanceof Error ? e.message : String(e);
    }
  });
  const updateSource = $(async (id: string) => {
    store.error = "";
    store.sourceState[id] = "updating";
    try {
      await updateToolSource(id);
      store.sourceState[id] = "updated";
    } catch (e) {
      store.sourceState[id] = "behind";
      store.error = e instanceof Error ? e.message : String(e);
    }
  });
  const installBlenderAddon = $(async () => {
    store.error = "";
    if (store.blenderAddonBusy !== "confirm") { store.blenderAddonBusy = "confirm"; return; }
    store.blenderAddonBusy = "installing";
    try {
      await blenderAddonInstall();
      store.blenderAddon = await blenderAddonStatus();
      store.blenderAddonBusy = "";
      store.note = "Blender add-on installed and enabled (and Blender's Allow Online Access turned on). If Blender is open, restart it once.";
    } catch (e) {
      store.blenderAddonBusy = "";
      store.error = e instanceof Error ? e.message : String(e);
    }
  });
  const recheckPreset = $(async (id: string) => {
    const preset = store.presets.find((p) => p.id === id);
    if (!preset) return;
    for (const n of preset.needs) store.have[n.program] = await whichProgram(n.program);
    if (!preset.needs.some((n) => store.have[n.program] === null)) { store.installFor = ""; store.installStage = ""; }
  });

  const addPreset = $(async (id: string) => {
    const preset = store.presets.find((p) => p.id === id);
    if (!preset) return;
    store.error = "";
    store.note = "";
    store.busy = id;
    try {
      if (preset.fetch) await fetchGit(preset.fetch.url, preset.fetch.dest);
      const built = preset.build();
      store.servers = await addMcpServer(built);
      if (built.config?.length) {
        store.note = `${preset.title} added - it needs a few settings first.`;
        await openConfig(built.name);
      } else {
        store.note = `${preset.title} added. Give it to an AI: Your AIs, edit, Tools.`;
      }
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    } finally {
      store.busy = "";
    }
  });

  const addManual = $(async () => {
    store.error = "";
    store.note = "";
    store.busy = "manual";
    try {
      const args = store.mArgs.trim() ? store.mArgs.trim().split(/\s+/) : [];
      store.servers = await addMcpServer({
        name: store.mName,
        description: store.mDescription.trim(),
        transport: store.mTransport,
        command: store.mTransport === "stdio" ? store.mCommand.trim() : undefined,
        args: store.mTransport === "stdio" ? args : [],
        env: [],
        url: store.mTransport === "http" ? store.mUrl.trim() : undefined,
        source: "manual",
        added_at: 0,
      });
      store.addOpen = false;
      store.mName = ""; store.mCommand = ""; store.mArgs = ""; store.mUrl = ""; store.mDescription = "";
      store.note = "Added. Give it to an AI: Your AIs, edit, Tools.";
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    } finally {
      store.busy = "";
    }
  });

  const remove = $(async (name: string) => {
    store.confirmRemove = "";
    store.error = "";
    try {
      store.servers = await removeMcpServer(name);
      // Drop it from every AI that carried it.
      for (const a of aiData.userDefinedAis) {
        if (Array.isArray(a.mcp) && a.mcp.includes(name)) {
          await editUserAi(a.id, { mcp: a.mcp.filter((n) => n !== name) });
        }
      }
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    }
  });

  const toggleAi = $(async (aiId: string, name: string) => {
    const a = aiData.userDefinedAis.find((x) => x.id === aiId);
    if (!a) return;
    const cur = Array.isArray(a.mcp) ? a.mcp : [];
    await editUserAi(aiId, { mcp: cur.includes(name) ? cur.filter((n) => n !== name) : [...cur, name] });
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
            onClick$={async () => { await nav("/add-ons"); }}
            class="inline-flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          >
            <LuChevronLeft class="h-4 w-4" /> Add-ons
          </button>

          <div class="mt-2 flex flex-col-reverse gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h1 class="flex items-center gap-2 text-2xl font-semibold text-[var(--text-primary)]">
                <LuWrench class="h-6 w-6 text-[var(--text-secondary)]" /> Tools
              </h1>
              <p class="mt-1 text-[var(--text-secondary)]">
                Programs your AIs can work in - Blender, a browser, a 3D printer, your smart home.
              </p>
            </div>
            <LiquidMetalButton
              onClick$={() => { store.addOpen = !store.addOpen; store.error = ""; }}
              class="shrink-0 flex items-center h-9 px-4 sm:px-5 text-[0.9375rem]"
            >
              Add your own
            </LiquidMetalButton>
          </div>

          <Callout intent="info" title="How tools work" id="tools-intro">
            A tool does nothing until you give it to an AI - "Used by" on a card here, or Your AIs, edit, Tools. For now
            tools work in projects: open a folder in a conversation with that AI and it can use them. Every action a tool
            takes goes through your approve step, the same as a file edit, and stays in your records.
          </Callout>

          {store.addOpen && (
            <div class="mt-4 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 space-y-3">
              <p class="text-sm text-[var(--text-secondary)]">
                Any MCP server: a program to run, or a local address it is already listening on.
              </p>
              <div class="grid gap-3 sm:grid-cols-2">
                <label class="text-xs text-[var(--text-secondary)]">
                  Name
                  <input type="text" value={store.mName} onInput$={(_, el) => { store.mName = el.value; }} placeholder="printer"
                    class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                </label>
                <label class="text-xs text-[var(--text-secondary)]">
                  What it is (optional)
                  <input type="text" value={store.mDescription} onInput$={(_, el) => { store.mDescription = el.value; }} placeholder="Our 3D printer"
                    class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                </label>
              </div>
              <div class="flex gap-4 text-sm text-[var(--text-primary)]">
                <label class="flex items-center gap-2"><input type="radio" checked={store.mTransport === "stdio"} onChange$={() => { store.mTransport = "stdio"; }} /> A program to run</label>
                <label class="flex items-center gap-2"><input type="radio" checked={store.mTransport === "http"} onChange$={() => { store.mTransport = "http"; }} /> A local address</label>
              </div>
              {store.mTransport === "stdio" ? (
                <div class="grid gap-3 sm:grid-cols-2">
                  <label class="text-xs text-[var(--text-secondary)]">
                    Program
                    <input type="text" value={store.mCommand} onInput$={(_, el) => { store.mCommand = el.value; }} placeholder="npx"
                      class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none font-mono" />
                  </label>
                  <label class="text-xs text-[var(--text-secondary)]">
                    Arguments
                    <input type="text" value={store.mArgs} onInput$={(_, el) => { store.mArgs = el.value; }} placeholder="-y @playwright/mcp"
                      class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none font-mono" />
                  </label>
                </div>
              ) : (
                <label class="block text-xs text-[var(--text-secondary)]">
                  Address (this computer only)
                  <input type="text" value={store.mUrl} onInput$={(_, el) => { store.mUrl = el.value; }} placeholder="http://127.0.0.1:9191/mcp"
                    class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none font-mono" />
                </label>
              )}
              <div class="flex justify-end gap-2">
                <LiquidMetalButton variant="secondary" onClick$={() => { store.addOpen = false; }} class="h-9 px-4 text-sm">Cancel</LiquidMetalButton>
                <LiquidMetalButton onClick$={addManual} disabled={store.busy === "manual" || !store.mName.trim()} class="h-9 px-4 text-sm">
                  {store.busy === "manual" ? "Adding..." : "Add"}
                </LiquidMetalButton>
              </div>
            </div>
          )}

          {store.error && (
            <p class="mt-4 flex items-start gap-2 text-sm text-red-400"><LuAlertTriangle class="mt-0.5 h-4 w-4 shrink-0" /> {store.error}</p>
          )}
          {store.note && <p class="mt-4 text-sm text-[var(--text-secondary)]">{store.note}</p>}

          {/* Presets - the ones we know how to set up */}
          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Ready to add</h2>
          <div class="mt-3 grid gap-4 sm:grid-cols-2">
            {store.presets.map((p) => {
              const pid = p.id;
              const installed = store.servers.some((s) => s.source === `preset:${pid}` || s.source === `directory:${pid}`);
              const missing = p.needs.filter((n) => store.have[n.program] === null);
              const checking = p.needs.some((n) => store.have[n.program] === undefined);
              return (
                <div key={p.id} class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3">
                  <div>
                    <h3 class="font-medium text-[var(--text-primary)]">{p.title}</h3>
                    <p class="mt-1 text-sm text-[var(--text-secondary)]">{p.blurb}</p>
                  </div>
                  <p class="text-xs text-[var(--text-muted)]">{p.notes}</p>
                  <ul class="text-xs space-y-1">
                    {p.needs.map((n) => (
                      <RequirementLine
                        key={n.program}
                        program={n.program}
                        label={n.label}
                        install={n.install}
                        have={store.have[n.program]}
                        onChange$={(v) => { store.have[n.program] = v; }}
                      />
                    ))}
                  </ul>
                  {pid === "blender" && installed && store.blenderAddon && (
                    <p class="flex flex-wrap items-center gap-1.5 text-xs">
                      {store.blenderAddon.installed ? (
                        <><LuCheck class="h-3.5 w-3.5 text-emerald-500" /><span class="text-[var(--text-secondary)]">Blender add-on installed</span></>
                      ) : !store.blenderAddon.blender ? (
                        <><LuAlertTriangle class="h-3.5 w-3.5 text-amber-500" /><span class="text-amber-500">Blender 5.1+ not found on this computer</span></>
                      ) : (
                        <>
                          <LuAlertTriangle class="h-3.5 w-3.5 text-amber-500" />
                          <span class="text-amber-500">Blender add-on not installed</span>
                          <button type="button" class="text-[var(--text-link)] hover:underline disabled:opacity-60" disabled={store.blenderAddonBusy === "installing"} onClick$={installBlenderAddon}>
                            {store.blenderAddonBusy === "installing" ? "Installing..." : store.blenderAddonBusy === "confirm" ? "Install it now" : "Install the add-on"}
                          </button>
                          {store.blenderAddonBusy === "confirm" && (
                            <span class="text-[var(--text-muted)]">Runs Blender's own extension installer on the add-on that came with the fetch, enables it, and turns on Blender's "Allow Online Access" setting - the add-on will not open its connection without it. That connection is local (this computer only); the setting is Blender's, and also lets Blender check its own extensions site for updates. Restart Blender afterwards if it is open.</span>
                          )}
                        </>
                      )}
                    </p>
                  )}
                  {p.fetch && !installed && (
                    <p class="text-xs text-[var(--text-muted)]">
                      Adding fetches the server ({p.fetch.size}) from {p.fetch.url.replace(/^https:\/\//, "").replace(/\.git$/, "")} into your home folder.
                    </p>
                  )}
                  {store.installFor === pid && (
                    <div class="rounded-xl border border-[var(--text-link)]/40 bg-[var(--bg-input)] p-3 text-xs">
                      <p class="font-medium text-[var(--text-primary)]">This will:</p>
                      <ul class="mt-1.5 space-y-1.5">
                        {store.installPlans.map((it) => (
                          <li key={it.program} class="text-[var(--text-secondary)]">
                            <span class="font-medium text-[var(--text-primary)]">{it.program}</span>
                            {" - "}
                            {it.plan.mode === "run" ? "runs here: " : it.plan.mode === "terminal" ? "opens your terminal (needs your password): " : "opens the download page: "}
                            {it.plan.note}{" "}
                            <code class="rounded bg-[var(--bg-card)] px-1 py-0.5 text-[10px] break-all">{it.plan.command || it.install}</code>
                          </li>
                        ))}
                      </ul>
                      {store.installStage && <p class="mt-2 text-amber-500">{store.installStage}</p>}
                      <div class="mt-3 flex flex-wrap gap-2">
                        <LiquidMetalButton onClick$={runInstallAll} disabled={!!store.busy} class="h-8 px-3 text-xs">
                          {store.busy === pid ? "Installing..." : "Install"}
                        </LiquidMetalButton>
                        <LiquidMetalButton variant="secondary" onClick$={() => recheckPreset(pid)} disabled={!!store.busy} class="h-8 px-3 text-xs">Check again</LiquidMetalButton>
                        <LiquidMetalButton variant="secondary" onClick$={() => { store.installFor = ""; store.installStage = ""; }} disabled={!!store.busy} class="h-8 px-3 text-xs">Cancel</LiquidMetalButton>
                      </div>
                    </div>
                  )}
                  <div class="flex items-center justify-between gap-2 mt-auto">
                    <span class="text-xs text-[var(--text-muted)]">
                      {installed ? "In your tools below - give it to an AI" : ""}
                    </span>
                    {installed ? (
                      <span class="inline-flex flex-wrap items-center gap-2 h-9 px-1 text-sm">
                        <span class="inline-flex items-center gap-1.5 text-emerald-500"><LuCheck class="h-4 w-4" /> Added</span>
                        {p.fetch && (() => {
                          const st = store.sourceState[pid] ?? "";
                          return st === "checking" ? (
                            <span class="text-xs text-[var(--text-muted)]">Checking...</span>
                          ) : st === "up-to-date" ? (
                            <span class="text-xs text-[var(--text-muted)]">Up to date</span>
                          ) : st === "behind" ? (
                            <button type="button" class="text-xs text-[var(--text-link)] hover:underline" onClick$={() => updateSource(pid)}>Update available - update</button>
                          ) : st === "updating" ? (
                            <span class="text-xs text-[var(--text-muted)]">Updating...</span>
                          ) : st === "updated" ? (
                            <span class="text-xs text-[var(--text-muted)]">Updated</span>
                          ) : (
                            <button type="button" class="text-xs text-[var(--text-link)] hover:underline" title="One check against the tool's source - nothing is checked unless you press this" onClick$={() => checkSource(pid)}>Check for updates</button>
                          );
                        })()}
                      </span>
                    ) : (
                      <LiquidMetalButton
                        disabled={!!store.busy || checking}
                        onClick$={() => (missing.length ? openInstallAll(pid) : addPreset(pid))}
                        class="flex items-center gap-1.5 h-9 px-4 text-sm"
                      >
                        {store.busy === p.id && <LuLoader2 class="h-4 w-4 animate-spin" />}
                        {missing.length ? "Install what it needs" : p.fetch ? "Fetch and add" : "Add"}
                      </LiquidMetalButton>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Installed */}
          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Your tools</h2>
          {store.loading ? (
            <p class="mt-3 text-sm text-[var(--text-muted)]">Loading...</p>
          ) : store.servers.length === 0 ? (
            <p class="mt-3 text-sm text-[var(--text-secondary)]">None yet. Add Blender above, or add your own.</p>
          ) : (
            <div class="mt-3 space-y-3">
              {store.servers.map((s) => {
                const users = mcpUsedBy(aiData.userDefinedAis, s.name);
                return (
                  <div key={s.name} class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-2">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <h3 class="font-medium text-[var(--text-primary)]">{s.name}</h3>
                        {s.description && <p class="text-sm text-[var(--text-secondary)]">{s.description}</p>}
                        <p class="text-xs text-[var(--text-muted)] font-mono truncate">{mcpSummary(s)}</p>
                      </div>
                      <div class="flex shrink-0 gap-2">
                        {s.source === "manual" && (
                          <LiquidMetalButton variant="secondary" onClick$={() => openShare(s)} class="flex items-center gap-1.5 px-3 py-1.5 text-xs" title="List this tool for everyone, signed with your Flowsta identity">
                            <LuShare2 class="h-3.5 w-3.5" /> Share
                          </LiquidMetalButton>
                        )}
                        {s.config?.length ? (
                          <LiquidMetalButton variant="secondary" onClick$={() => openConfig(s.name)} class="flex items-center gap-1.5 px-3 py-1.5 text-xs">
                            Settings
                          </LiquidMetalButton>
                        ) : null}
                        <LiquidMetalButton variant="secondary" onClick$={() => { store.confirmRemove = s.name; }} class="flex items-center gap-1.5 px-3 py-1.5 text-xs">
                          <LuTrash2 class="h-3.5 w-3.5" /> Remove
                        </LiquidMetalButton>
                      </div>
                    </div>
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
                    {missingSettings(s.name).length > 0 && (
                      <p class="flex items-center gap-1.5 text-xs text-amber-500">
                        <LuAlertTriangle class="h-3.5 w-3.5" /> Needs its settings before an AI can use it: {missingSettings(s.name).join(", ")}
                      </p>
                    )}
                    <button
                      type="button"
                      onClick$={() => { store.usedByOpen = store.usedByOpen === s.name ? "" : s.name; }}
                      class="inline-flex items-center gap-1.5 text-left text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
                      title="Choose which AIs carry this tool"
                    >
                      <LuUsers class="h-3.5 w-3.5 shrink-0" />
                      Used by: {users.length ? users.join(", ") : "no AI yet - choose one"}
                    </button>
                    {store.usedByOpen === s.name && (
                      <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] p-3 space-y-1.5">
                        {aiData.userDefinedAis
                          .filter((a) => a.status === "active")
                          .map((a) => {
                            const on = Array.isArray(a.mcp) && a.mcp.includes(s.name);
                            return (
                              <label key={a.id} class="flex items-center gap-2 text-sm text-[var(--text-primary)]">
                                <input type="checkbox" checked={on} onChange$={() => toggleAi(a.id, s.name)} />
                                {a.name}
                              </label>
                            );
                          })}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {store.shareFor && (
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
          <div class="w-full max-w-md rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
            <h3 class="text-base font-semibold text-[var(--text-primary)]">Share "{store.shareFor}" with everyone</h3>
            {store.shareDone ? (
              <div class="mt-3 space-y-3 text-sm text-[var(--text-secondary)]">
                <p>Submitted, signed with your Flowsta identity. After a quick look it lives at <span class="text-[var(--text-primary)] break-all">{store.shareDone.page}</span></p>
                <LiquidMetalButton variant="secondary" class="w-full justify-center px-5 py-2 text-sm" onClick$={() => { store.shareFor = ""; }}>Done</LiquidMetalButton>
              </div>
            ) : (
              <>
                <p class="mt-2 text-sm text-[var(--text-secondary)]">
                  Lists the recipe - how to start it, what it needs, the settings it asks for - never your settings or their values. It goes out signed with your Flowsta identity and is yours to update or remove.
                </p>
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Name in the listing</label>
                <input type="text" value={store.shareTitle} onInput$={(_, el) => { store.shareTitle = el.value; }} maxLength={60} class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Description</label>
                <textarea value={store.shareDescription} onInput$={(_, el) => { store.shareDescription = el.value; }} rows={3} maxLength={400} class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-xl px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Where it comes from (https link)</label>
                <input type="text" value={store.shareSource} onInput$={(_, el) => { store.shareSource = el.value; }} placeholder="https://github.com/…" class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Anything people need to know first (optional)</label>
                <input type="text" value={store.shareAlso} onInput$={(_, el) => { store.shareAlso = el.value; }} placeholder="Needs the app running with…" maxLength={240} class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none" />
                <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">License</label>
                <select value={store.shareLicense} onChange$={(_, el) => { store.shareLicense = el.value; }} class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)]">
                  {LICENSES.map((l) => (<option key={l.id} value={l.id}>{l.label}</option>))}
                </select>
                <p class="mt-3 text-xs text-[var(--text-muted)]">
                  {store.shareMaker ? `Listed as @${store.shareMaker} and signed with your Flowsta identity, so people know it is yours.` : "Sign in with Flowsta first - the listing shows who made it."}
                </p>
                {store.shareErr && <p class="mt-2 text-xs text-red-400">{store.shareErr}</p>}
                <div class="mt-4 flex flex-col gap-2">
                  <LiquidMetalButton onClick$={doShare} disabled={store.shareBusy || !store.shareMaker} class="w-full justify-center px-5 py-2 text-sm">
                    {store.shareBusy ? "Signing and sending..." : "Share"}
                  </LiquidMetalButton>
                  <LiquidMetalButton variant="secondary" onClick$={() => { store.shareFor = ""; }} disabled={store.shareBusy} class="w-full justify-center px-5 py-2 text-sm">Cancel</LiquidMetalButton>
                </div>
              </>
            )}
          </div>
        </div>
      )}

      {store.configFor && (() => {
        const s = store.servers.find((x) => x.name === store.configFor);
        return (
          <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
            <div class="w-full max-w-md rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
              <h3 class="text-base font-semibold text-[var(--text-primary)]">Settings for {s?.name}</h3>
              <p class="mt-1 text-xs text-[var(--text-muted)]">
                Kept on this computer only. Secrets are stored encrypted and are sent only to this tool.
              </p>
              <div class="mt-4 space-y-3">
                {(s?.config ?? []).map((f) => {
                  const key = f.key;
                  const filled = store.configOk[store.configFor]?.[key];
                  return (
                    <label key={key} class="block text-xs text-[var(--text-secondary)]">
                      {f.label || f.key}{f.required ? "" : " (optional)"}
                      {f.kind === "secret" && filled && <span class="ml-2 text-emerald-500">set - leave blank to keep</span>}
                      <input
                        type={f.kind === "secret" ? "password" : "text"}
                        value={store.configDraft[key] ?? ""}
                        onInput$={(_, el) => { store.configDraft[key] = el.value; }}
                        placeholder={f.hint ?? ""}
                        autocomplete="off"
                        class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
                      />
                    </label>
                  );
                })}
              </div>
              <div class="mt-5 flex justify-end gap-2">
                <LiquidMetalButton variant="secondary" onClick$={() => { store.configFor = ""; }} disabled={store.configSaving} class="h-9 px-4 text-sm">Cancel</LiquidMetalButton>
                <LiquidMetalButton onClick$={saveConfig} disabled={store.configSaving} class="h-9 px-4 text-sm">{store.configSaving ? "Saving..." : "Save"}</LiquidMetalButton>
              </div>
            </div>
          </div>
        );
      })()}

      <ConfirmModal
        isOpen={!!store.confirmRemove}
        title="Remove this tool?"
        message={`"${store.confirmRemove}" is removed from the list and from every AI that carried it. Nothing else on your computer changes.`}
        confirmLabel="Remove"
        variant="danger"
        onConfirm$={() => remove(store.confirmRemove)}
        onCancel$={() => { store.confirmRemove = ""; }}
      />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Tools - Your Own AI",
};
