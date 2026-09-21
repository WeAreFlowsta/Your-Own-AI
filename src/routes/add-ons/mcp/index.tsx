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
import { LuWrench, LuTrash2, LuChevronLeft, LuAlertTriangle, LuCheck, LuChevronDown } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData, useAiDataActions } from "../../../contexts/AiDataContext";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import ConfirmModal from "../../../components/ConfirmModal";
import { Callout } from "../../../components/Callout";
import { ToolSetup } from "../../../components/ToolSetup";
import { useBuildInstall } from "../../../hooks/useBuildInstall";
import { LICENSES, currentMaker, shareTool, shareErrorText, type ShareResult } from "../../../utils/share";
import { rememberShare, rememberedShare, fetchShareStatus, shareStatusText, type ShareStatus } from "../../../utils/shareStatus";
import { LuShare2 } from "@qwikest/icons/lucide";
import {
  listMcpServers,
  addMcpServer,
  removeMcpServer,
  whichProgram,
  fetchGit,
  checkToolSource,
  updateToolSource,
  keepVaultInSync,
  mcpSummary,
  readyPresets,
  toolConfigStatus,
  toolReadiness,
  withCardData,
  type McpPreset,
  type McpServer,
} from "../../../utils/mcp";

/** Programs a shared tool may start through (the directory's rule) - they verify what they fetch. */
const SHARE_LAUNCHERS = ["uv", "uvx", "npx", "pipx", "docker", "python", "python3", "node", "deno", "bunx"];


/** The add-on a yourownai:// link asked for (handed over by the layout's
 *  deep-link handler): read once, then scroll its card into view and ring
 *  it for a moment. Nothing happens when the id is not on this page. */
function takeAddOnFocus(): string {
  try {
    const id = sessionStorage.getItem("addOnFocusId") ?? "";
    if (id) sessionStorage.removeItem("addOnFocusId");
    return id;
  } catch {
    return "";
  }
}
function scrollToAddOn(id: string, clear: () => void) {
  setTimeout(() => {
    document.getElementById(`addon-${id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    setTimeout(clear, 4000);
  }, 80);
}

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();
  const { editUserAi } = useAiDataActions();
  const build = useBuildInstall();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);
  const store = useStore({
    /** Card named by an incoming yourownai:// link - ringed until it fades. */
    focus: "" as string,
    servers: [] as McpServer[],
    presets: [] as McpPreset[],
    loading: true,
    error: "",
    note: "",
    busy: "" as string, // preset id or "manual" while adding
    // preset readiness: program -> path | null (checked on open)
    have: {} as Record<string, string | null>,
    /** The row in Your Tools whose Set up list is open. */
    rowOpen: "" as string,
    inventoryOpen: true,
    // explicit source checks: preset id -> "checking" | "up-to-date" | "behind" | "updating" | "updated"
    sourceState: {} as Record<string, string>,
    confirmRemove: "" as string,
    // which settings hold a value: tool name -> key -> filled
    configOk: {} as Record<string, Record<string, boolean>>,
    // the tool's own download is on this computer: tool name -> yes / no
    fetched: {} as Record<string, boolean>,
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
    shareLicenseOpen: false,
    // the launcher rule the directory applies - checked here before signing
    shareLauncherOk: true,
    shareLauncher: "",
    shareStatus: {} as Record<string, ShareStatus>,
    addOpen: false,
    // manual add form
    mName: "",
    mTransport: "stdio" as "stdio" | "http",
    mCommand: "",
    mArgs: "",
    mUrl: "",
    mDescription: "",
  });

  // What a row's status line reads: which settings are filled, and whether
  // the tool's own download is here.
  const refreshTool = $(async (name: string) => {
    try { store.configOk[name] = await toolConfigStatus(name); } catch { /* shown as unfilled */ }
    try { store.fetched[name] = (await toolReadiness(name)).fetched; } catch { /* no line */ }
  });
  const load = $(async () => {
    store.servers = await listMcpServers();
    store.loading = false;
    for (const s of store.servers) {
      await refreshTool(s.name);
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
    store.shareLicenseOpen = false;
    const launcher = (s.command ?? "").trim().split(/\s+/)[0].split(/[\\/]/).pop() ?? "";
    store.shareLauncher = launcher;
    store.shareLauncherOk = s.transport === "http" || SHARE_LAUNCHERS.includes(launcher);
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

  // Settings saved in a Set up list: take the new list, re-read the tool.
  const saved = $(async (name: string, servers: McpServer[]) => {
    store.servers = servers;
    await refreshTool(name);
  });
  // Open a tool's row in Your Tools and bring it into view.
  const showRow = $((name: string) => {
    store.inventoryOpen = true;
    store.rowOpen = name;
    store.focus = name;
    setTimeout(() => {
      document.getElementById(`tool-${name}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
      setTimeout(() => { store.focus = ""; }, 4000);
    }, 80);
  });
  const missingSettings = (name: string): string[] => {
    const raw = store.servers.find((x) => x.name === name);
    const s = raw ? withCardData(raw) : undefined;
    const ok = store.configOk[name] ?? {};
    return (s?.config ?? []).filter((f) => f.required && !ok[f.key]).map((f) => f.label || f.key);
  };

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    await load();
    store.presets = await readyPresets();
    // A card id (a yourownai:// link) or a tool's name (the chat's tools
    // chip). An added tool opens its row in Your Tools; otherwise its card.
    const asked = takeAddOnFocus();
    const added = store.servers.find((s) => s.name === asked || s.source === `preset:${asked}` || s.source === `directory:${asked}`);
    if (added) {
      await showRow(added.name);
    } else if (asked && store.presets.some((p) => p.id === asked)) {
      store.focus = asked;
      scrollToAddOn(asked, () => { store.focus = ""; });
    }
    const programs = new Set<string>();
    for (const p of store.presets) for (const n of p.needs) programs.add(n.program);
    for (const prog of programs) store.have[prog] = await whichProgram(prog);
  });

  const handleNewQuestion = $(() => { nav("/chat"); });
  const handleModelsClick = $(() => { nav("/setup"); });

  // Add puts the tool in Your Tools and opens its Set up list. It fetches
  // and installs NOTHING - every download is a line in that list, with its
  // own button.
  const addPreset = $(async (id: string) => {
    const preset = store.presets.find((p) => p.id === id);
    if (!preset) return;
    store.error = "";
    store.note = "";
    store.busy = id;
    try {
      const built = preset.build();
      store.servers = await addMcpServer(built);
      await refreshTool(built.name);
      await showRow(built.name);
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
    } finally {
      store.busy = "";
    }
  });
  // The tool's own download, from its row's Set up list.
  const fetchTool = $(async (name: string) => {
    const s = store.servers.find((x) => x.name === name);
    const card = store.presets.find((p) => s?.source === `preset:${p.id}` || s?.source === `directory:${p.id}`);
    if (!card?.fetch) return;
    await fetchGit(card.fetch.url, card.fetch.dest);
    await refreshTool(name);
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

  const addManual = $(async () => {
    store.error = "";
    store.note = "";
    store.busy = "manual";
    try {
      // "Program" may hold the whole line (npx -y @playwright/mcp@latest):
      // first word = program, the rest lead the arguments.
      const line = store.mCommand.trim().split(/\s+/).filter(Boolean);
      const args = [...line.slice(1), ...(store.mArgs.trim() ? store.mArgs.trim().split(/\s+/) : [])];
      store.servers = await addMcpServer({
        name: store.mName,
        description: store.mDescription.trim(),
        transport: store.mTransport,
        command: store.mTransport === "stdio" ? line[0] ?? "" : undefined,
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
    const adding = !cur.includes(name);
    await editUserAi(aiId, { mcp: adding ? [...cur, name] : cur.filter((n) => n !== name) });
    // A notes tool that also remembers its vault: the AI that now carries it
    // gets the vault in its documents. (Taking the tool away leaves the
    // documents - "Stop syncing" on the AI's documents is the way out.)
    const s = store.servers.find((x) => x.name === name);
    if (adding && s) {
      const n = await keepVaultInSync(s, [aiId]).catch(() => 0);
      if (n) store.note = `${a.name} can use ${name}, and its notes folder is being read into that AI's documents and kept in sync.`;
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
                Let your AIs work inside other programs - Blender, Obsidian, a browser, your smart home.
              </p>
            </div>
            <LiquidMetalButton
              onClick$={() => { store.addOpen = !store.addOpen; store.error = ""; }}
              class="shrink-0 flex items-center h-9 px-4 sm:px-5 text-[0.9375rem]"
            >
              Add your own
            </LiquidMetalButton>
          </div>

          <Callout intent="info" title="What tools are" id="tools-what">
            Tools let your AI work in other programs, like Blender or Obsidian. Add one, set it up, then ask your AI
            in chat. It asks before each action.
          </Callout>

          {!build.installed.value && store.servers.length > 0 && (
            <p class="mt-4 text-sm text-[var(--text-secondary)]">
              Your AIs use tools through Your Own AI Build, a free add-on that is not installed yet.{" "}
              {build.downloading.value ? (
                <span class="text-[var(--text-muted)]">Downloading... {build.percent.value}%</span>
              ) : (
                <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={build.install$}>Install it</button>
              )}
              {build.error.value && <span class="ml-2 text-red-400">{build.error.value}</span>}
            </p>
          )}

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
                    <input type="text" value={store.mCommand} onInput$={(_, el) => { store.mCommand = el.value; }} placeholder="npx (or paste the whole line)"
                      class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none font-mono" />
                  </label>
                  <label class="text-xs text-[var(--text-secondary)]">
                    Arguments
                    <input type="text" value={store.mArgs} onInput$={(_, el) => { store.mArgs = el.value; }} placeholder="-y @playwright/mcp@latest"
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

          {/* Your tools - the inventory: compact rows, the Set up list opens in the row */}
          {(store.loading || store.servers.length > 0) && (
            <div class="mt-8">
              <button
                type="button"
                onClick$={() => { store.inventoryOpen = !store.inventoryOpen; }}
                class="flex w-full items-center justify-between mb-4 border-b border-[var(--border-subtle)] pb-2 bg-transparent border-x-0 border-t-0 cursor-pointer text-left"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela">
                  Your Tools{store.loading ? "" : ` (${store.servers.length})`}
                </h2>
                <LuChevronDown class={`h-5 w-5 text-[var(--text-muted)] transition-transform ${store.inventoryOpen ? "" : "-rotate-90"}`} />
              </button>
              {store.loading ? (
                <p class="text-sm text-[var(--text-muted)]">Loading...</p>
              ) : store.inventoryOpen && (
                <div class="generic-container rounded-2xl divide-y divide-[var(--border-subtle)]">
                  {store.servers.map((raw) => {
                    const s = withCardData(raw);
                    const users = aiData.userDefinedAis.filter((x) => x.status === "active" && Array.isArray(x.mcp) && x.mcp.includes(s.name));
                    const acting = users.filter((x) => !(Array.isArray(x.mcpOff) && x.mcpOff.includes(s.name)));
                    const fromCard = s.source.replace(/^(preset|directory):/, "");
                    const card = fromCard !== s.source ? store.presets.find((p) => p.id === fromCard) : undefined;
                    const unset = missingSettings(s.name);
                    const open = store.rowOpen === s.name;
                    // One line: the first thing standing between this tool and an AI using it.
                    const status = !build.installed.value
                      ? { ok: false, text: "Your Own AI Build is not installed" }
                      : store.fetched[s.name] === false
                        ? { ok: false, text: "Not fetched yet" }
                      : unset.length
                        ? { ok: false, text: `Needs its settings: ${unset.join(", ")}` }
                        : users.length === 0
                          ? { ok: false, text: "No AI uses it yet" }
                          : acting.length === 0
                            ? { ok: false, text: `Switched off in chat for ${users.map((x) => x.name).join(", ")}` }
                            : { ok: true, text: `Ready - used by ${acting.map((x) => x.name).join(", ")}` };
                    return (
                      <div key={s.name} id={`tool-${s.name}`} class={`p-4 transition-shadow ${store.focus === s.name ? "ring-2 ring-[var(--text-link)] rounded-2xl" : ""}`}>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                          <div class="min-w-0">
                            <h3 class="font-medium text-[var(--text-primary)]">{card?.title ?? s.name}</h3>
                            {s.description && <p class="text-sm text-[var(--text-secondary)]">{s.description}</p>}
                            <p class={`mt-1 flex items-center gap-1.5 text-xs ${status.ok ? "text-[var(--text-secondary)]" : "text-amber-600 dark:text-amber-400"}`}>
                              {status.ok ? <LuCheck class="h-3.5 w-3.5 shrink-0 text-emerald-500" /> : <LuAlertTriangle class="h-3.5 w-3.5 shrink-0" />}
                              {status.text}
                            </p>
                            {!card && <p class="mt-1 text-xs text-[var(--text-muted)] font-mono truncate">{mcpSummary(s)}</p>}
                          </div>
                          <div class="flex shrink-0 flex-wrap gap-2">
                            {s.source === "manual" && (
                              <LiquidMetalButton variant="secondary" onClick$={() => openShare(s)} class="flex items-center gap-1.5 px-3 py-1.5 text-xs" title="List this tool for everyone, signed with your Flowsta identity">
                                <LuShare2 class="h-3.5 w-3.5" /> Share
                              </LiquidMetalButton>
                            )}
                            <LiquidMetalButton variant="secondary" onClick$={() => { store.confirmRemove = s.name; }} class="flex items-center gap-1.5 px-3 py-1.5 text-xs">
                              <LuTrash2 class="h-3.5 w-3.5" /> Remove
                            </LiquidMetalButton>
                            <LiquidMetalButton
                              variant={status.ok ? "secondary" : undefined}
                              onClick$={() => { store.rowOpen = open ? "" : s.name; }}
                              class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                            >
                              Set up
                              <LuChevronDown class={`h-3.5 w-3.5 transition-transform ${open ? "rotate-180" : ""}`} />
                            </LiquidMetalButton>
                          </div>
                        </div>
                        {store.shareStatus[s.name] && (
                          <p class="mt-2 text-xs text-[var(--text-secondary)]">
                            <span class="font-medium text-[var(--text-primary)]">Shared with everyone: </span>
                            {shareStatusText(store.shareStatus[s.name], s.name)}{" "}
                            <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={async () => {
                              const st = store.shareStatus[s.name];
                              const { openUrl } = await import("@tauri-apps/plugin-opener");
                              await openUrl(st.state === "live" ? st.page : st.pr_url);
                            }}>{store.shareStatus[s.name].state === "live" ? "Open the page" : "See the submission"}</button>
                          </p>
                        )}
                        {open && (
                          <ToolSetup
                            title={card?.title ?? s.name}
                            needs={card?.needs ?? []}
                            fetch={card?.fetch}
                            server={s}
                            ais={aiData.userDefinedAis}
                            have={store.have}
                            onHave$={(program, v) => { store.have[program] = v; }}
                            onFetch$={() => fetchTool(s.name)}
                            onToggleAi$={(aiId) => toggleAi(aiId, s.name)}
                            onSaved$={(servers) => saved(s.name, servers)}
                          />
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {/* Presets - the ones we know how to set up */}
          <h2 class="mt-8 mb-4 border-b border-[var(--border-subtle)] pb-2 text-2xl font-bold text-[var(--text-primary)] font-varela">Available Tools</h2>
          <div class="grid gap-4 sm:grid-cols-2">
            {store.presets.map((p) => {
              const pid = p.id;
              const mine = store.servers.find((s) => s.source === `preset:${pid}` || s.source === `directory:${pid}`);
              const installed = !!mine;
              return (
                <div key={p.id} id={`addon-${p.id}`} class={`rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3 transition-shadow ${store.focus === p.id ? "ring-2 ring-[var(--text-link)]" : ""}`}>
                  <div>
                    <h3 class="font-medium text-[var(--text-primary)]">{p.title}</h3>
                    <p class="mt-1 text-sm text-[var(--text-secondary)]">{p.blurb}</p>
                  </div>
                  <p class="text-xs text-[var(--text-muted)]">{p.notes}</p>
                  {p.needs.length > 0 && (
                    <p class="text-xs text-[var(--text-muted)]">Runs on {p.needs.map((n) => n.program).join(" and ")} - its Set up list checks for them.</p>
                  )}
                  <div class="flex items-center justify-between gap-2 mt-auto">
                    <span class="inline-flex flex-wrap items-center gap-2 text-xs text-[var(--text-muted)]">
                      {installed ? (
                        <span class="inline-flex items-center gap-1.5 text-sm text-emerald-500"><LuCheck class="h-4 w-4" /> Added</span>
                      ) : (
                        ""
                      )}
                      {installed && p.fetch && (() => {
                        const st = store.sourceState[pid] ?? "";
                        return st === "checking" ? (
                          <span>Checking...</span>
                        ) : st === "up-to-date" ? (
                          <span>Up to date</span>
                        ) : st === "behind" ? (
                          <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={() => updateSource(pid)}>Update available - update</button>
                        ) : st === "updating" ? (
                          <span>Updating...</span>
                        ) : st === "updated" ? (
                          <span>Updated</span>
                        ) : (
                          <button type="button" class="text-[var(--text-link)] hover:underline" title="One check against the tool's source - nothing is checked unless you press this" onClick$={() => checkSource(pid)}>Check for updates</button>
                        );
                      })()}
                    </span>
                    {installed ? (
                      <button
                        type="button"
                        class="text-sm text-[var(--text-link)] hover:underline"
                        onClick$={() => { if (mine) showRow(mine.name); }}
                      >
                        Set up in Your Tools
                      </button>
                    ) : (
                      <LiquidMetalButton
                        disabled={!!store.busy}
                        onClick$={() => addPreset(pid)}
                        class="flex items-center gap-1.5 h-9 px-4 text-sm"
                        title="Adds it to Your Tools. Nothing is downloaded until you press a line in its Set up list."
                      >
                        Add
                      </LiquidMetalButton>
                    )}
                  </div>
                </div>
              );
            })}
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
                <p>An AI reviewer reads it against the directory rules and posts what it finds, then a person on the Your Own AI team decides. Updates to your own listing go through on their own once the checks pass.</p>
                <p>Once it is listed it lives at <span class="text-[var(--text-primary)] break-all">{store.shareDone.page}</span> - the card here shows where it stands.</p>
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
                {!store.shareLauncherOk && (
                  <p class="mt-3 text-xs text-amber-500">
                    This tool starts with "{store.shareLauncher}". Shared tools must start through uv, npx, pipx, docker, python, node, deno or bunx - programs that fetch and verify their own packages - so this one can't be listed as it is.
                  </p>
                )}
                <p class="mt-3 text-xs text-[var(--text-muted)]">
                  {store.shareMaker ? `Listed as @${store.shareMaker} and signed with your Flowsta identity, so people know it is yours.` : "Sign in with Flowsta first - the listing shows who made it."}
                </p>
                {store.shareErr && <p class="mt-2 text-xs text-red-400">{store.shareErr}</p>}
                <div class="mt-4 flex justify-end gap-2">
                  <LiquidMetalButton variant="secondary" onClick$={() => { store.shareFor = ""; }} disabled={store.shareBusy} class="h-9 px-5 text-sm">Cancel</LiquidMetalButton>
                  <LiquidMetalButton onClick$={doShare} disabled={store.shareBusy || !store.shareMaker || !store.shareLauncherOk} class="h-9 px-5 text-sm">
                    {store.shareBusy ? "Signing and sending..." : "Share"}
                  </LiquidMetalButton>
                </div>
              </>
            )}
          </div>
        </div>
      )}

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
