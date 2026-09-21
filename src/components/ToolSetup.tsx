import { component$, useStore, useTask$, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { LuCheck, LuCircle, LuLoader2, LuAlertTriangle, LuInfo } from '@qwikest/icons/lucide';
import { RequirementLine } from './RequirementLine';
import { ToolSettingsFields } from './ToolSettingsFields';
import LiquidMetalButton from './LiquidMetalButton';
import { useBuildInstall } from '../hooks/useBuildInstall';
import {
  withCardData,
  toolReadiness,
  checkPort,
  toolConfigStatus,
  setToolConfig,
  blenderAddonStatus,
  blenderAddonInstall,
  keepVaultInSync,
  unsavedSettings,
  type McpServer,
  type BlenderAddonStatus,
  type Readiness,
} from '../utils/mcp';
import type { UserDefinedAI } from '../types';

export interface ToolSetupProps {
  title: string;
  needs: { program: string; label: string; install: string }[];
  /** The tool's own download, when the app fetches it (a clone). */
  fetch?: { url: string; dest: string; size: string };
  /** The tool, in the list. Adding it fetched nothing - line 3 does. */
  server: McpServer;
  ais: UserDefinedAI[];
  have: Record<string, string | null>;
  onHave$: QRL<(program: string, have: string | null) => void>;
  /** The tool's own download. */
  onFetch$: QRL<() => Promise<void>>;
  onToggleAi$: QRL<(aiId: string) => Promise<void>>;
  /** Settings saved: the page reloads its copy of the tool. */
  onSaved$: QRL<(servers: McpServer[]) => void>;
}

type LineState = 'done' | 'todo' | 'busy' | 'warn' | 'info';

const Mark = component$<{ state: LineState }>(({ state }) =>
  state === 'done' ? (
    <LuCheck class="h-4 w-4 shrink-0 text-emerald-500" />
  ) : state === 'busy' ? (
    <LuLoader2 class="h-4 w-4 shrink-0 animate-spin text-[var(--text-muted)]" />
  ) : state === 'warn' ? (
    <LuAlertTriangle class="h-4 w-4 shrink-0 text-amber-500" />
  ) : state === 'info' ? (
    <LuInfo class="h-4 w-4 shrink-0 text-[var(--text-muted)]" />
  ) : (
    <LuCircle class="h-4 w-4 shrink-0 text-[var(--text-muted)]" />
  ),
);

/**
 * Setting a tool up, as ONE list. Each line says where it stands and has its
 * own button; nothing downloads or installs without that line's button being
 * pressed. The lines come from the card's own data (what it needs, what it
 * fetches, its settings, the checks it declares) - nothing here knows which
 * tool it is.
 *
 *  1. Your Own AI Build - the helper that runs tools. First on every list:
 *     it is what people miss.
 *  2. The programs the tool needs.
 *  3. The tool itself (a fetch now, or a plain note when a launcher fetches
 *     it on first use).
 *  4. Its settings.
 *  5. Which AI uses it - and whether it is switched off for that AI in chat.
 *  6. Checks the card declares.
 *  Ends on Ready, or on exactly what is still missing.
 */
export const ToolSetup = component$<ToolSetupProps>((props) => {
  const build = useBuildInstall();
  const server = withCardData(props.server);
  const state = useStore({
    adding: false,
    addError: '',
    draft: {} as Record<string, string>,
    filled: {} as Record<string, boolean>,
    saving: false,
    saved: '',
    settingsError: '',
    /** Short names of the settings that differ from what is stored. */
    unsaved: [] as string[],
    readiness: null as Readiness | null,
    ports: {} as Record<number, boolean>,
    blender: null as BlenderAddonStatus | null,
    blenderBusy: false,
    blenderConfirm: false,
    blenderNote: '',
  });
  const name = server.name;

  // The actions read the tool from the props as they are NOW: the tool first
  // exists after "Add", long after these closures were made.
  const refresh = $(async () => {
    const name = props.server?.name ?? '';
    if (!name) return;
    try {
      state.filled = await toolConfigStatus(name);
    } catch { /* no settings */ }
    try {
      state.readiness = await toolReadiness(name);
    } catch {
      state.readiness = null;
    }
  });

  const runChecks = $(async () => {
    const checks = props.server ? (withCardData(props.server).checks ?? []) : [];
    for (const c of checks) {
      if (c.kind === 'port') state.ports[c.port] = await checkPort(c.port).catch(() => false);
      if (c.kind === 'helper' && c.helper === 'blender-addon') {
        state.blender = await blenderAddonStatus().catch(() => null);
      }
    }
  });

  // What differs from what is stored. Watched here, over the WHOLE form and
  // the stored values at once - worked out while drawing, a change could be
  // missed and "Not saved yet" outlive the edit that caused it.
  useTask$(({ track }) => {
    track(() => JSON.stringify(state.draft));
    track(() => JSON.stringify(props.server.values ?? {}));
    state.unsaved = unsavedSettings(withCardData(props.server).config ?? [], state.draft, props.server.values);
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => props.server?.name);
    track(() => JSON.stringify(props.server?.values ?? {}));
    const now = props.server;
    if (!now) return;
    for (const f of now.config ?? []) {
      if (state.draft[f.key] === undefined) {
        state.draft[f.key] = f.kind === 'secret' ? '' : (now.values?.[f.key] ?? f.default ?? (f.kind === 'toggle' ? 'off' : ''));
      }
    }
    await refresh();
    await runChecks();
  });

  const fetchIt = $(async () => {
    state.adding = true;
    state.addError = '';
    try {
      await props.onFetch$();
      await refresh();
    } catch (e) {
      state.addError = e instanceof Error ? e.message : String(e);
    } finally {
      state.adding = false;
    }
  });

  const save = $(async () => {
    const name = props.server?.name ?? '';
    if (!name) return;
    state.saving = true;
    state.settingsError = '';
    state.saved = '';
    try {
      const values: Record<string, string> = {};
      for (const [k, v] of Object.entries(state.draft)) if (v.trim()) values[k] = v.trim();
      const servers = await setToolConfig(name, values);
      // A saved secret never comes back: empty its box, so the field reads
      // "set" and nothing looks unsaved.
      for (const f of props.server.config ?? []) if (f.kind === 'secret') state.draft[f.key] = '';
      await props.onSaved$(servers);
      const mine = servers.find((s) => s.name === name);
      const users = props.ais.filter((a) => Array.isArray(a.mcp) && a.mcp.includes(name)).map((a) => a.id);
      const remembering = mine ? await keepVaultInSync(mine, users).catch(() => 0) : 0;
      state.saved = remembering
        ? 'Saved. The folder is being read into the documents of the AIs that use this tool, and will be kept in sync.'
        : 'Saved on this computer.';
      await refresh();
    } catch (e) {
      state.settingsError = e instanceof Error ? e.message : String(e);
    } finally {
      state.saving = false;
    }
  });

  const installBlenderAddon = $(async () => {
    if (!state.blenderConfirm) {
      state.blenderConfirm = true;
      return;
    }
    state.blenderBusy = true;
    state.blenderNote = '';
    try {
      await blenderAddonInstall();
      state.blenderNote = "Installed and enabled (and Blender's Allow Online Access turned on). If Blender is open, restart it once.";
    } catch (e) {
      state.blenderNote = e instanceof Error ? e.message : String(e);
    } finally {
      state.blenderBusy = false;
      state.blenderConfirm = false;
      state.blender = await blenderAddonStatus().catch(() => null);
    }
  });

  const missingPrograms = props.needs.filter((n) => props.have[n.program] === null);
  const programsChecking = props.needs.some((n) => props.have[n.program] === undefined);
  const fetched = state.readiness?.fetched ?? true;
  const config = server.config ?? [];
  const missingSettings = config.filter((f) => f.required && !state.filled[f.key]).map((f) => f.label || f.key);
  const unsaved = state.unsaved.length > 0;
  const users = props.ais.filter((a) => a.status === 'active' && Array.isArray(a.mcp) && a.mcp.includes(name));
  const resting = users.filter((a) => Array.isArray(a.mcpOff) && a.mcpOff.includes(name));
  const acting = users.length - resting.length;

  const btn = 'text-[var(--text-link)] hover:underline disabled:opacity-60 cursor-pointer';
  const row = 'flex items-start gap-2.5 py-2';
  const head = 'text-sm text-[var(--text-primary)]';
  const sub = 'text-xs text-[var(--text-muted)]';

  // What still stands between this tool and an AI using it.
  const blockers: string[] = [];
  if (!build.installed.value) blockers.push('Your Own AI Build is not installed');
  if (missingPrograms.length) blockers.push(`${missingPrograms.map((n) => n.program).join(', ')} not found`);
  if (!fetched) blockers.push('the tool is not fetched yet');
  if (unsaved) blockers.push(`not saved yet (${state.unsaved.join(', ')}) - press Save settings`);
  else if (missingSettings.length) {
    blockers.push(`${missingSettings.join(', ')} ${missingSettings.length === 1 ? 'is' : 'are'} missing - fill ${missingSettings.length === 1 ? 'it' : 'them'} in above and press Save settings`);
  }
  if (users.length === 0) blockers.push('no AI uses it yet');
  if (users.length > 0 && acting === 0) blockers.push('it is switched off in chat for every AI that has it');
  if (fetched && state.readiness && !state.readiness.ready && !missingSettings.length) blockers.push(state.readiness.reason);

  return (
    <div class="mt-3 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 divide-y divide-[var(--border-subtle)]">
      {/* 1. Your Own AI Build */}
      <div class={row}>
        <Mark state={build.downloading.value ? 'busy' : build.installed.value ? 'done' : 'todo'} />
        <div class="min-w-0 flex-1">
          <p class={head}>Your Own AI Build</p>
          <p class={sub}>
            The helper that lets your AIs use tools and work in project folders. A free add-on, about 50 MB.
          </p>
          {build.downloading.value && (
            <p class={sub}>Downloading... {build.percent.value}% - keeps going if you leave this page.</p>
          )}
          {build.error.value && <p class="text-xs text-red-500">{build.error.value}</p>}
        </div>
        {!build.installed.value && !build.downloading.value && (
          <button type="button" class={`${btn} shrink-0 text-xs`} onClick$={build.install$}>Install</button>
        )}
      </div>

      {/* 2. What the tool needs */}
      {props.needs.length > 0 && (
        <div class={row}>
          <Mark state={programsChecking ? 'busy' : missingPrograms.length ? 'todo' : 'done'} />
          <div class="min-w-0 flex-1">
            <p class={head}>What {props.title} needs</p>
            <ul class="mt-1 space-y-1 text-xs">
              {props.needs.map((n) => (
                <RequirementLine
                  key={n.program}
                  program={n.program}
                  label={n.label}
                  install={n.install}
                  have={props.have[n.program]}
                  onChange$={(v) => props.onHave$(n.program, v)}
                />
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* 3. The tool itself: its own download, or a plain note when a launcher
          fetches it on first use. Nothing to say for a tool with neither. */}
      {(props.fetch || server.first_use) && (
        <div class={row}>
          <Mark state={state.adding ? 'busy' : !props.fetch ? 'info' : fetched ? 'done' : 'todo'} />
          <div class="min-w-0 flex-1">
            <p class={head}>{props.title} itself</p>
            {props.fetch ? (
              <p class={sub}>
                {fetched ? 'Fetched' : 'Fetches the tool'} ({props.fetch.size}) from{' '}
                {props.fetch.url.replace(/^https?:\/\//, '').replace(/\.git$/, '')} into your home folder.
              </p>
            ) : (
              <p class={sub}>{server.first_use}</p>
            )}
            {state.addError && <p class="text-xs text-red-500">{state.addError}</p>}
          </div>
          {props.fetch && !fetched && state.readiness && (
            <button
              type="button"
              class={`${btn} shrink-0 text-xs`}
              disabled={state.adding || programsChecking || missingPrograms.length > 0}
              title={missingPrograms.length ? 'Install what it needs first' : undefined}
              onClick$={fetchIt}
            >
              {state.adding ? 'Fetching...' : 'Fetch'}
            </button>
          )}
        </div>
      )}

      {/* 4. Its settings */}
      {config.length > 0 && (
        <div class={row}>
          <Mark state={state.saving ? 'busy' : unsaved ? 'warn' : missingSettings.length ? 'todo' : 'done'} />
          <div class="min-w-0 flex-1">
            <p class={head}>Settings</p>
            <p class={sub}>Kept on this computer only. Secrets are stored encrypted and are sent only to this tool.</p>
            <div class="mt-2">
              <ToolSettingsFields config={config} draft={state.draft} filled={state.filled} />
            </div>
            <div class="mt-3 flex flex-wrap items-center gap-3 text-xs">
              <LiquidMetalButton
                variant={unsaved ? undefined : 'secondary'}
                disabled={state.saving}
                onClick$={save}
                class="h-8 px-4 text-xs"
              >
                {state.saving ? 'Saving...' : 'Save settings'}
              </LiquidMetalButton>
              {unsaved && !state.saving && (
                <span class="text-amber-600 dark:text-amber-400">Not saved yet: {state.unsaved.join(', ')}</span>
              )}
              {!unsaved && state.saved && <span class="text-[var(--text-muted)]">{state.saved}</span>}
              {state.settingsError && <span class="text-red-500">{state.settingsError}</span>}
            </div>
          </div>
        </div>
      )}

      {/* 5. Which AI uses it */}
      {(
        <div class={row}>
          <Mark state={users.length === 0 ? 'todo' : acting === 0 ? 'warn' : 'done'} />
          <div class="min-w-0 flex-1">
            <p class={head}>Which AI uses it</p>
            <div class="mt-1 space-y-1">
              {props.ais
                .filter((a) => a.status === 'active')
                .map((a) => {
                  const on = Array.isArray(a.mcp) && a.mcp.includes(name);
                  const off = on && Array.isArray(a.mcpOff) && a.mcpOff.includes(name);
                  return (
                    <label key={a.id} class="flex items-center gap-2 text-sm text-[var(--text-primary)] cursor-pointer">
                      <input type="checkbox" class="cursor-pointer" checked={on} onChange$={() => props.onToggleAi$(a.id)} />
                      {a.name}
                      {off && (
                        <span class="text-xs text-amber-600 dark:text-amber-400">
                          - switched off in chat (the tools chip beside the message field)
                        </span>
                      )}
                    </label>
                  );
                })}
            </div>
          </div>
        </div>
      )}

      {/* 6. Checks the card declares */}
      {(server.checks ?? []).map((c, i) =>
          c.kind === 'port' ? (
            <div key={`port-${c.port}`} class={row}>
              <Mark state={state.ports[c.port] ? 'done' : 'todo'} />
              <div class="min-w-0 flex-1">
                <p class={head}>{state.ports[c.port] ? c.ok : c.missing}</p>
              </div>
              <button type="button" class={`${btn} shrink-0 text-xs`} onClick$={runChecks}>Check again</button>
            </div>
          ) : c.kind === 'helper' && c.helper === 'blender-addon' ? (
            <div key={`helper-${i}`} class={row}>
              <Mark
                state={
                  state.blenderBusy ? 'busy' : !state.blender ? 'todo' : state.blender.installed && state.blender.listening ? 'done' : state.blender.installed ? 'warn' : 'todo'
                }
              />
              <div class="min-w-0 flex-1">
                <p class={head}>The add-on inside Blender</p>
                <p class={sub}>
                  {!state.blender
                    ? 'Checking...'
                    : !state.blender.blender
                      ? 'Blender 5.1+ not found on this computer.'
                      : state.blender.installed && state.blender.listening
                        ? 'Installed and running - your AI can reach Blender.'
                        : state.blender.installed
                          ? 'Installed, not running - open Blender (or restart it if it was open during the install).'
                          : fetched
                            ? 'Not installed yet.'
                            : 'Not installed yet - it comes with the fetch above.'}
                </p>
                {state.blenderConfirm && (
                  <p class={`${sub} mt-1`}>
                    Runs Blender's own extension installer on the add-on that came with the fetch, enables it, and turns on
                    Blender's "Allow Online Access" setting - the add-on will not open its connection without it. That
                    connection is local (this computer only); the setting is Blender's, and also lets Blender check its own
                    extensions site for updates. Restart Blender afterwards if it is open.
                  </p>
                )}
                {state.blenderNote && <p class={sub}>{state.blenderNote}</p>}
              </div>
              <div class="shrink-0 text-xs flex flex-col items-end gap-1">
                {/* the add-on comes with the tool's own download */}
                {state.blender?.blender && !state.blender.installed && fetched && (
                  <button type="button" class={btn} disabled={state.blenderBusy} onClick$={installBlenderAddon}>
                    {state.blenderBusy ? 'Installing...' : state.blenderConfirm ? 'Install it now' : 'Install the add-on'}
                  </button>
                )}
                <button type="button" class={btn} onClick$={runChecks}>Check again</button>
              </div>
            </div>
          ) : null,
        )}

      {/* Ready, or exactly what is missing */}
      <div class={row}>
        <Mark state={blockers.length === 0 ? 'done' : 'info'} />
        <div class="min-w-0 flex-1">
          {blockers.length === 0 ? (
            <p class={head}>
              Ready - ask {users.filter((a) => !resting.includes(a)).map((a) => a.name).join(' or ')} to use {props.title}.
            </p>
          ) : (
            <p class={sub}>Not ready yet: {blockers.join('; ')}.</p>
          )}
          {!build.installed.value && server.sync && (
            <p class={sub}>
              Remembering the folder works without Your Own AI Build; it is only needed for your AI to act on it.
            </p>
          )}
        </div>
      </div>
    </div>
  );
});
