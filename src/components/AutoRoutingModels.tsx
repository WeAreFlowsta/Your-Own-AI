import { component$, useStore, useVisibleTask$, $ } from '@builder.io/qwik';
import { LuChevronRight } from '@qwikest/icons/lucide';
import { invoke } from '@tauri-apps/api/core';
import {
  AUTO_ROUTING_CHANGED,
  autoAllowedMap,
  providerAllows,
  readAutoAllow,
  setModelAllowed,
  setProviderAllowed,
  type AutoAllow,
  type OnlineProvider,
} from '../utils/autoRouting';

type Model = { id: string; display_name: string; provider?: string | null };

/**
 * Settings > Routing: "Models automatic routing may use". One row per
 * provider, collapsed (name, where it is based, how many of its models are
 * on, its switch); open a row for a switch per model. Where a provider is
 * based and whether it starts on come from the catalog, never from a name
 * in this file. A model picked by name for a kind of question is used
 * either way - this list governs only what routing chooses by itself.
 */
export const AutoRoutingModels = component$(() => {
  const st = useStore<{
    providers: OnlineProvider[];
    models: Model[];
    allowed: Record<string, boolean>;
    switches: AutoAllow;
    open: Record<string, boolean>;
    loaded: boolean;
  }>({ providers: [], models: [], allowed: {}, switches: { providers: {}, models: {} }, open: {}, loaded: false });

  const reload = $(async () => {
    const [providers, models, allowed, switches] = await Promise.all([
      invoke<OnlineProvider[]>('list_online_providers').catch(() => []),
      invoke<Model[]>('list_online_models').catch(() => []),
      autoAllowedMap(),
      readAutoAllow(),
    ]);
    st.providers = providers;
    st.models = models;
    st.allowed = allowed;
    st.switches = switches;
    st.loaded = true;
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    void reload();
    const on = () => void reload();
    window.addEventListener(AUTO_ROUTING_CHANGED, on);
    cleanup(() => window.removeEventListener(AUTO_ROUTING_CHANGED, on));
  });

  // An older catalog has no providers block: every model counts as allowed
  // and there is nothing to switch.
  if (!st.loaded || st.providers.length === 0) return null;

  return (
    <div class="mt-5" data-testid="auto-routing-models">
      <h4 class="text-base font-semibold text-[var(--text-primary)]">Models automatic routing may use</h4>
      <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
        Routing picks from these by itself: the model for each kind of question, and a stand-in from
        another provider when one is busy or down. A model you choose above is used either way.
      </p>
      <div class="rounded-xl border border-[var(--border-subtle)] divide-y divide-[var(--border-subtle)] bg-[var(--bg-main)]">
        {st.providers.map((p) => {
          const models = st.models.filter((m) => m.provider === p.id);
          const on = models.filter((m) => st.allowed[m.id] !== false).length;
          const providerOn = providerAllows(p, st.switches);
          const open = !!st.open[p.id];
          return (
            <div key={p.id} data-provider={p.id}>
              <div class="flex items-center gap-3 px-3 py-2.5">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-2 text-left"
                  aria-expanded={open}
                  onClick$={() => {
                    st.open = { ...st.open, [p.id]: !open };
                  }}
                >
                  <LuChevronRight class={`h-4 w-4 flex-shrink-0 text-[var(--text-muted)] transition-transform ${open ? 'rotate-90' : ''}`} />
                  <span class="min-w-0 flex flex-wrap items-baseline gap-x-2">
                    <span class="text-sm font-medium text-[var(--text-primary)]">{p.name}</span>
                    {p.based_in && <span class="text-xs text-[var(--text-muted)]">Based in {p.based_in}</span>}
                  </span>
                  <span class="ml-auto flex-shrink-0 text-xs text-[var(--text-muted)] tabular-nums">
                    {on} of {models.length} {models.length === 1 ? 'model' : 'models'}
                  </span>
                </button>
                <button
                  type="button"
                  role="switch"
                  aria-checked={providerOn}
                  aria-label={`Automatic routing may use ${p.name}'s models`}
                  data-testid="auto-provider-switch"
                  class={`relative inline-block h-5 w-9 flex-shrink-0 rounded-full transition-colors ${providerOn ? 'bg-[var(--text-link)]' : 'bg-[var(--border-subtle)]'}`}
                  onClick$={async () => {
                    await setProviderAllowed(p, models.map((m) => m.id), !providerOn);
                  }}
                >
                  <span class={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${providerOn ? 'left-[18px]' : 'left-0.5'}`} />
                </button>
              </div>
              {open && (
                <div class="pb-2 pl-9 pr-3">
                  {models.map((m) => {
                    const allowed = st.allowed[m.id] !== false;
                    return (
                      <div key={m.id} class="flex items-center justify-between gap-3 py-1.5">
                        <span class="min-w-0 truncate text-sm text-[var(--text-secondary)]">{m.display_name}</span>
                        <button
                          type="button"
                          role="switch"
                          aria-checked={allowed}
                          aria-label={`Automatic routing may use ${m.display_name}`}
                          data-testid="auto-model-switch"
                          class={`relative inline-block h-4 w-7 flex-shrink-0 rounded-full transition-colors ${allowed ? 'bg-[var(--text-link)]' : 'bg-[var(--border-subtle)]'}`}
                          onClick$={async () => {
                            await setModelAllowed(m.id, providerOn, !allowed);
                          }}
                        >
                          <span class={`absolute top-0.5 h-3 w-3 rounded-full bg-white shadow transition-all ${allowed ? 'left-3.5' : 'left-0.5'}`} />
                        </button>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});
