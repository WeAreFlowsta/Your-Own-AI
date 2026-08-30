import { component$, useSignal, useStore, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { useNavigate } from '@builder.io/qwik-city';
import { LuCheck, LuChevronDown, LuSparkles, LuWrench } from '@qwikest/icons/lucide';
import { useAiDataActions } from '../contexts/AiDataContext';
import type { UserDefinedAI } from '../types';
import { activeSkills, activeTools } from '../utils/carry';
import { PERMISSION_MODE_COPY, type AgentPermissionMode } from '../utils/agentPermissions';

/**
 * The chip beside the model chip: what this AI carries - tools and skills -
 * as an icon and a count each, and nothing at all when it carries nothing.
 * Opens one panel: every tool and skill with an on/off switch, approvals
 * for tools sessions, and the way to Add-ons. Off keeps a tool on the AI
 * (Add-ons still says "used by"); it just sits out. All tools off = the AI
 * answers directly, no agent machinery.
 */
interface CarryChipProps {
  ai: UserDefinedAI;
  /** Open the panel upward (the chat's bottom input bar). */
  dropUp?: boolean;
  /** Projects, the helper that runs tools: installed, installing, or missing. */
  buildInstalled: boolean;
  installing?: boolean;
  installPercent?: number;
  /** Approvals for this AI's tools sessions; undefined = not choosable here. */
  permissionMode?: AgentPermissionMode;
  /** A tools turn is running right now - the chip pulses. */
  live?: boolean;
  /** The AI was patched (persisted here); hosts holding a snapshot sync it. */
  onChanged$: QRL<(patch: Partial<UserDefinedAI>) => void>;
  onPermission$?: QRL<(mode: AgentPermissionMode) => void>;
  onInstall$?: QRL<() => void>;
}

export const CarryChip = component$<CarryChipProps>((props) => {
  const actions = useAiDataActions();
  const nav = useNavigate();
  const open = useSignal(false);
  const panel = useStore({ left: 0, top: 0, bottom: 0 });
  // Same teleport as the model chip: the panel is position:fixed and moved
  // to document.body while open, so no ancestor can clip it.
  const overlayRef = useSignal<HTMLElement>();
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    track(() => open.value);
    const el = overlayRef.value;
    if (open.value && el && el.parentElement !== document.body) {
      document.body.appendChild(el);
    }
    cleanup(() => {
      if (el && el.parentElement === document.body) el.remove();
    });
  });

  const toggleTool = $(async (name: string) => {
    const off = new Set(props.ai.mcpOff ?? []);
    if (off.has(name)) off.delete(name);
    else off.add(name);
    const mcpOff = [...off];
    await actions.updateCustomAi(props.ai.id, { mcpOff });
    await props.onChanged$({ mcpOff });
  });
  const toggleSkill = $(async (name: string) => {
    const off = new Set(props.ai.skillsOff ?? []);
    if (off.has(name)) off.delete(name);
    else off.add(name);
    const skillsOff = [...off];
    await actions.updateCustomAi(props.ai.id, { skillsOff });
    await props.onChanged$({ skillsOff });
  });

  const tools = props.ai.mcp ?? [];
  const skills = props.ai.skills ?? [];
  if (!tools.length && !skills.length) return null;
  const onTools = activeTools(props.ai);
  const onSkills = activeSkills(props.ai);
  const needsProjects = tools.length > 0 && !props.buildInstalled;

  const rowClass = 'flex items-center justify-between gap-3 px-4 py-1.5 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] cursor-default select-none';
  const label = 'select-none pb-1 pl-4 pr-4 pt-2 text-xs uppercase tracking-wider text-[var(--text-muted)]';
  const Switch = (on: boolean) => (
    <span
      role="switch"
      aria-checked={on}
      class={`relative inline-block h-4 w-7 flex-shrink-0 rounded-full transition-colors ${on ? 'bg-[var(--text-link)]' : 'bg-[var(--border-subtle)]'}`}
    >
      <span class={`absolute top-0.5 h-3 w-3 rounded-full bg-white shadow transition-all ${on ? 'left-3.5' : 'left-0.5'}`} />
    </span>
  );

  return (
    <div class="relative inline-block max-w-full text-left">
      <button
        type="button"
        class="inline-flex max-w-full items-center gap-1.5 text-[11px] sm:text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
        title="What this AI carries - tools and skills, on or off"
        onClick$={(_, el) => {
          open.value = !open.value;
          if (open.value) {
            const r = el.getBoundingClientRect();
            panel.left = Math.max(8, Math.min(r.left, window.innerWidth - 296));
            panel.top = r.bottom + 4;
            panel.bottom = window.innerHeight - r.top + 4;
          }
        }}
      >
        {tools.length > 0 && (
          <span class="inline-flex items-center gap-0.5">
            <LuWrench class={`h-3 w-3 ${props.live ? 'animate-pulse text-[var(--text-link)]' : ''}`} />
            <span class="hidden sm:inline">{onTools.length}</span>
          </span>
        )}
        {tools.length > 0 && skills.length > 0 && <span class="hidden sm:inline">·</span>}
        {skills.length > 0 && (
          <span class="inline-flex items-center gap-0.5">
            <LuSparkles class="h-3 w-3" />
            <span class="hidden sm:inline">{onSkills.length}</span>
          </span>
        )}
        {needsProjects && (
          <span class="h-1.5 w-1.5 rounded-full bg-amber-500" title="Tools need Projects, a free helper - open to install it" />
        )}
        <LuChevronDown class="h-3 w-3 flex-shrink-0" />
      </button>
      {open.value && (
        <div ref={overlayRef}>
          <div class="fixed inset-0 z-20" onClick$={() => (open.value = false)} />
          <div
            class="fixed z-30 w-72 max-h-80 overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 text-sm shadow-lg ring-1 ring-black ring-opacity-5"
            style={
              props.dropUp
                ? { left: `${panel.left}px`, bottom: `${panel.bottom}px` }
                : { left: `${panel.left}px`, top: `${panel.top}px` }
            }
          >
            {tools.length > 0 && (
              <>
                <div class={label}>Tools</div>
                {tools.map((name) => {
                  const on = onTools.includes(name);
                  return (
                    <div key={name} class={rowClass} onClick$={() => toggleTool(name)}>
                      <span class={`truncate ${on ? 'text-[var(--text-primary)]' : ''}`}>{name}</span>
                      {Switch(on)}
                    </div>
                  );
                })}
                {needsProjects && (
                  <div class="px-4 py-2 text-xs text-[var(--text-muted)]">
                    Tools need Projects, a free ~50 MB helper.{' '}
                    {props.installing ? (
                      <span>Installing... {props.installPercent ?? 0}%</span>
                    ) : (
                      <button
                        type="button"
                        class="text-[var(--text-link)] hover:underline"
                        onClick$={() => { props.onInstall$?.(); }}
                      >
                        Install it
                      </button>
                    )}
                  </div>
                )}
                {props.permissionMode && props.onPermission$ && (
                  <>
                    <div class={label}>Approvals</div>
                    {(['ask', 'auto', 'all'] as AgentPermissionMode[]).map((mode) => {
                      const selected = props.permissionMode === mode;
                      return (
                        <div
                          key={mode}
                          class={`relative cursor-default select-none py-1.5 pl-9 pr-3 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${selected ? 'text-[var(--text-primary)]' : 'text-[var(--text-dropdown)]'}`}
                          title={PERMISSION_MODE_COPY[mode].hint}
                          onClick$={() => { props.onPermission$?.(mode); }}
                        >
                          <span class={`block truncate ${selected ? 'font-medium' : 'font-normal'}`}>{PERMISSION_MODE_COPY[mode].label}</span>
                          {selected && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-4 w-4" aria-hidden="true" />
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </>
                )}
              </>
            )}
            {skills.length > 0 && (
              <>
                <div class={`${label} ${tools.length ? 'mt-1 border-t border-[var(--border-subtle)]' : ''}`}>Skills</div>
                {skills.map((name) => {
                  const on = onSkills.includes(name);
                  return (
                    <div key={name} class={rowClass} onClick$={() => toggleSkill(name)}>
                      <span class={`truncate ${on ? 'text-[var(--text-primary)]' : ''}`}>{name}</span>
                      {Switch(on)}
                    </div>
                  );
                })}
              </>
            )}
            <div class="mt-1 flex items-center justify-between gap-3 border-t border-[var(--border-subtle)] px-4 pb-1 pt-2 text-xs text-[var(--text-muted)]">
              <button
                type="button"
                class="text-[var(--text-link)] hover:underline"
                onClick$={async () => { open.value = false; await nav('/add-ons/'); }}
              >
                Get more in Add-ons
              </button>
              <span>Applies from your next message</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
