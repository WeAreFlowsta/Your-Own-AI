import { component$, useSignal, useStore, $, type QRL } from '@builder.io/qwik';
import { LuCheck, LuChevronDown } from '@qwikest/icons/lucide';
import { invoke } from '@tauri-apps/api/core';
import {
  getCachedModels,
  refreshLocalModels,
  refreshFits,
  refreshOnlineModels,
  type FitMap,
  type OnlineModel,
} from '../utils/modelCache';
import { formatModelForCard, richModelName } from '../utils/modelNameFormatter';
import { isModelPaused } from '../utils/modelPrefs';
import { autoOptions, offeredOnlineModels } from '../utils/modelOptions';
import { useAiDataActions } from '../contexts/AiDataContext';
import type { LocalModel, UpdateUserAiData } from '../types';

/**
 * The model line that IS the switcher. Shows an AI's current Base Model
 * and opens a compact picker in place - so changing the model happens
 * where people look for it (the AI's card, the chat's Ask row) instead
 * of only inside the edit modal. Renders from the same rules as the
 * modal picker (utils/modelOptions) and persists through the same
 * context action, so all surfaces stay in agreement.
 */
interface ModelChipProps {
  aiId: string;
  model: string;
  /** card = on a Your AIs card; header = the chat's Ask row (quieter). */
  variant: 'card' | 'header';
  /** Open the panel upward (the chat's bottom input bar). */
  dropUp?: boolean;
  /** Extra sync for hosts holding their own copy of the AI (the chat
   *  patches its selected-AI snapshot); persistence itself is handled
   *  here. */
  onChanged$?: QRL<(model: string) => void>;
}

export const ModelChip = component$<ModelChipProps>((props) => {
  const actions = useAiDataActions();
  const open = useSignal(false);
  // Panel position in viewport coordinates. The panel is position:fixed and
  // measured from the trigger, NOT absolute inside the chip - the chip can
  // sit inside overflow-hidden containers (the AI card's truncation wrapper
  // clips absolute children into invisibility).
  const panel = useStore({ left: 0, top: 0, bottom: 0 });
  const store = useStore({
    loading: false,
    localModels: [] as LocalModel[],
    onlineModels: [] as OnlineModel[],
    externalModels: [] as string[],
    onlineEntitled: true,
    fits: {} as FitMap,
  });

  const load = $(async () => {
    if (store.loading) return;
    store.loading = true;
    // Instant from the session cache, revalidate in the background - the
    // same stale-while-revalidate the modal uses.
    const cached = getCachedModels();
    if (cached.local) store.localModels = cached.local;
    if (cached.online) store.onlineModels = cached.online;
    if (cached.fits) store.fits = cached.fits;
    refreshLocalModels()
      .then((m) => {
        store.localModels = m;
      })
      .catch(() => {})
      .finally(() => {
        store.loading = false;
      });
    refreshFits()
      .then((f) => {
        store.fits = f;
      })
      .catch(() => {});
    refreshOnlineModels()
      .then((m) => {
        store.onlineModels = m;
      })
      .catch(() => {});
    invoke<{ healthy: boolean; models: string[] }>('external_engine_info')
      .then((i) => {
        store.externalModels = i.healthy ? i.models : [];
      })
      .catch(() => {
        store.externalModels = [];
      });
    import('../utils/entitlement')
      .then(({ getOnlineEntitlement }) => getOnlineEntitlement())
      .then((e) => {
        store.onlineEntitled = e.entitled;
      })
      .catch(() => {});
  });

  const select = $(async (model: string) => {
    open.value = false;
    if (model === props.model) return;
    await actions.updateCustomAi(props.aiId, { model } as UpdateUserAiData);
    if (props.onChanged$) await props.onChanged$(model);
  });

  const itemClass = (selected: boolean) =>
    `relative cursor-default select-none py-2 pl-9 pr-3 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
      selected
        ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
        : 'text-[var(--text-dropdown)]'
    }`;

  return (
    <div class="relative inline-block max-w-full text-left">
      <button
        type="button"
        class={`inline-flex max-w-full items-center gap-1 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors ${
          props.variant === 'header' ? 'text-[11px] sm:text-xs' : ''
        }`}
        title="Change which model answers as this AI"
        onClick$={async (_, el) => {
          open.value = !open.value;
          if (open.value) {
            const r = el.getBoundingClientRect();
            panel.left = Math.max(8, Math.min(r.left, window.innerWidth - 296));
            panel.top = r.bottom + 4;
            panel.bottom = window.innerHeight - r.top + 4;
            await load();
          }
        }}
      >
        <span class="truncate">{formatModelForCard(props.model)}</span>
        <LuChevronDown class="w-3 h-3 flex-shrink-0" />
      </button>
      {open.value && (
        <>
          <div class="fixed inset-0 z-20" onClick$={() => (open.value = false)} />
          <div
            class="fixed z-30 w-72 max-h-64 overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 text-sm shadow-lg ring-1 ring-black ring-opacity-5"
            style={
              props.dropUp
                ? { left: `${panel.left}px`, bottom: `${panel.bottom}px` }
                : { left: `${panel.left}px`, top: `${panel.top}px` }
            }
          >
            {store.loading && store.localModels.length === 0 ? (
              <div class="flex items-center gap-2 px-4 py-3 text-xs text-[var(--text-muted)]">
                <span class="w-3.5 h-3.5 border-2 border-[var(--text-muted)] border-t-transparent rounded-full animate-spin" />
                Loading your models…
              </div>
            ) : (
              <>
                <div class="select-none pb-1 pl-4 pr-4 pt-1 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                  Smart routing
                </div>
                {autoOptions({
                  hasExternal: store.externalModels.length > 0,
                  hasOnlineModels: store.onlineModels.length > 0,
                  onlineEntitled: store.onlineEntitled,
                  currentModel: props.model,
                }).map((opt) => (
                  <div key={opt.id} class={itemClass(props.model === opt.id)} onClick$={() => select(opt.id)}>
                    <span class={`block truncate ${props.model === opt.id ? 'font-medium' : 'font-normal'}`}>
                      {opt.label}
                      <span class="ml-2 text-xs text-[var(--text-muted)]">{opt.hint}</span>
                    </span>
                    {props.model === opt.id && (
                      <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                        <LuCheck class="h-4 w-4" aria-hidden="true" />
                      </span>
                    )}
                  </div>
                ))}
                {store.localModels.length > 0 && (
                  <div class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                    Your offline models
                  </div>
                )}
                {store.localModels
                  .filter((m) => !isModelPaused(m.name) || m.name === props.model)
                  .map((m) => (
                    <div key={m.name} class={itemClass(props.model === m.name)} onClick$={() => select(m.name)}>
                      <span class={`block truncate ${props.model === m.name ? 'font-medium' : 'font-normal'}`}>
                        {store.fits[m.name] && (
                          <span
                            class={`inline-block w-2 h-2 rounded-full mr-2 align-middle ${
                              store.fits[m.name] === 'green'
                                ? 'bg-green-500'
                                : store.fits[m.name] === 'yellow'
                                  ? 'bg-yellow-500'
                                  : 'bg-red-500'
                            }`}
                          />
                        )}
                        {richModelName(m.name)}
                      </span>
                      {props.model === m.name && (
                        <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                          <LuCheck class="h-4 w-4" aria-hidden="true" />
                        </span>
                      )}
                    </div>
                  ))}
                {store.onlineModels.length > 0 && !store.onlineEntitled && (
                  <div class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-2 pl-4 pr-4 text-xs text-[var(--text-muted)]">
                    Online models (and Auto — Online and Offline) unlock with a plan
                    - set up on the Online Models page.
                  </div>
                )}
                {store.onlineModels.length > 0 && store.onlineEntitled && (
                  <div class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                    Online models
                  </div>
                )}
                {offeredOnlineModels(store.onlineModels, store.onlineEntitled, props.model, isModelPaused).map(
                  (om) => (
                    <div key={om.id} class={itemClass(props.model === om.id)} onClick$={() => select(om.id)}>
                      <span class={`block truncate ${props.model === om.id ? 'font-medium' : 'font-normal'}`}>
                        {om.display_name}
                      </span>
                      {props.model === om.id && (
                        <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                          <LuCheck class="h-4 w-4" aria-hidden="true" />
                        </span>
                      )}
                    </div>
                  ),
                )}
                {store.externalModels.length > 0 && (
                  <div class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                    Your server
                  </div>
                )}
                {store.externalModels.map((name) => (
                  <div
                    key={name}
                    class={itemClass(props.model === `external:${name}`)}
                    onClick$={() => select(`external:${name}`)}
                  >
                    <span class={`block truncate ${props.model === `external:${name}` ? 'font-medium' : 'font-normal'}`}>
                      {name}
                    </span>
                    {props.model === `external:${name}` && (
                      <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                        <LuCheck class="h-4 w-4" aria-hidden="true" />
                      </span>
                    )}
                  </div>
                ))}
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
});
