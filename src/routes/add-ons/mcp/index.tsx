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
import { LuWrench, LuTrash2, LuLoader, LuChevronLeft, LuUsers, LuAlertTriangle } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData, useAiDataActions } from "../../../contexts/AiDataContext";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import ConfirmModal from "../../../components/ConfirmModal";
import { Callout } from "../../../components/Callout";
import { RequirementLine } from "../../../components/RequirementLine";
import {
  listMcpServers,
  addMcpServer,
  removeMcpServer,
  whichProgram,
  fetchGit,
  mcpUsedBy,
  mcpSummary,
  readyPresets,
  setToolConfig,
  toolConfigStatus,
  type McpPreset,
  type McpServer,
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
    confirmRemove: "" as string,
    // settings form: which tool, the draft values, and what is filled in
    configFor: "" as string,
    configDraft: {} as Record<string, string>,
    configOk: {} as Record<string, Record<string, boolean>>,
    configSaving: false,
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
    for (const s of store.servers) {
      if (s.config?.length) {
        try { store.configOk[s.name] = await toolConfigStatus(s.name); } catch { /* shown as unfilled */ }
      }
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
                  {p.fetch && !installed && (
                    <p class="text-xs text-[var(--text-muted)]">
                      Adding fetches the server ({p.fetch.size}) from {p.fetch.url.replace(/^https:\/\//, "").replace(/\.git$/, "")} into your home folder.
                    </p>
                  )}
                  <div class="flex items-center justify-between gap-2 mt-auto">
                    <span class="text-xs text-[var(--text-muted)]">
                      {installed ? "Added" : ""}
                    </span>
                    <LiquidMetalButton
                      variant={installed ? "secondary" : "primary"}
                      disabled={!!store.busy || checking || missing.length > 0}
                      onClick$={() => addPreset(pid)}
                      class="flex items-center gap-1.5 h-9 px-4 text-sm"
                    >
                      {store.busy === p.id && <LuLoader class="h-4 w-4 animate-spin" />}
                      {installed ? "Update" : missing.length ? "Install what it needs first" : p.fetch ? "Fetch and add" : "Add"}
                    </LiquidMetalButton>
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
