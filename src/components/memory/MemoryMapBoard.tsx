/**
 * The Map: what memory has kept, by subject. Circles sized by how much each
 * subject holds, lines between subjects that come up together, a ring that
 * says who can use it. About you is always there, from the first day. The
 * same content is one press away as a list, and pressing a subject shows
 * what it holds.
 */
import { component$, useSignal, useVisibleTask$, $, noSerialize, type NoSerialize } from '@builder.io/qwik';
import LiquidMetalButton from '../LiquidMetalButton';
import { ago, audienceWords, countWords, loadMap, renameSubject, type MapSubject, type MemoryMapData } from '../../utils/memoryMap';
import type { MapHandle } from '../../utils/memoryMapDraw';
import type { KeptItem } from '../../utils/memoryKeep';
import { dayOf } from '../../utils/memoryKeep';

interface Ai {
  id: string;
  name: string;
  founding: string;
}

const LIVE = ['current', 'open', 'done'];
const PANEL_ITEMS = 6;

/** `aiId`: the AI whose view is shown (its id on this device), chosen with
 *  the faces above the Map; absent or "you" for all the person's AIs. */
export const MemoryMapBoard = component$<{ aiId?: string }>((props) => {
  const data = useSignal<MemoryMapData | null>(null);
  const failed = useSignal('');
  const ais = useSignal<Ai[]>([]);
  const viewAi = useSignal('');
  const viewProject = useSignal('');
  const mode = useSignal<'map' | 'list'>('map');
  const selected = useSignal<string | null>(null);
  const held = useSignal<KeptItem[]>([]);
  const allShown = useSignal(false);
  const naming = useSignal(false);
  const nameDraft = useSignal('');
  const stage = useSignal<HTMLDivElement>();
  const handle = useSignal<NoSerialize<MapHandle>>();

  const load = $(async () => {
    try {
      data.value = await loadMap({ ai: viewAi.value, project: viewProject.value });
      failed.value = '';
    } catch (e) {
      failed.value = String(e);
    }
  });

  // The AIs to choose from, the first load, and a new load whenever memory changes.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const [{ getLocalCustomAis }, { foundingOf }] = await Promise.all([import('../../utils/localAiStorage'), import('../../utils/lookalikes')]);
    const list = await getLocalCustomAis().catch(() => []);
    ais.value = list.filter((a) => a.status !== 'archived' && foundingOf(a)).map((a) => ({ id: a.id, name: a.name, founding: foundingOf(a) }));
    viewAi.value = ais.value.find((a) => a.id === props.aiId)?.founding ?? '';
    await load();
    const again = () => void load();
    window.addEventListener('yoai-memory-changed', again);
    cleanup(() => window.removeEventListener('yoai-memory-changed', again));
  });

  // The face chosen above the Map changes whose view is shown.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const id = track(() => props.aiId);
    const founding = ais.value.find((a) => a.id === id)?.founding ?? '';
    if (founding === viewAi.value) return;
    viewAi.value = founding;
    void load();
  });

  // The drawing, mounted once.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const el = stage.value;
    if (!el) return;
    const { mountMap } = await import('../../utils/memoryMapDraw');
    const h = await mountMap(el, { select: (id) => (selected.value = id) });
    handle.value = noSerialize(h);
    // For pressing a circle without a pointer (the drawing is one canvas).
    (window as unknown as { __yoaiMap?: unknown }).__yoaiMap = { placed: () => h.placed(), select: (id: string | null) => (selected.value = id) };
    cleanup(() => {
      delete (window as unknown as { __yoaiMap?: unknown }).__yoaiMap;
      h.destroy();
    });
  });

  // What is drawn follows the data and the chosen project.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const [d, h] = [track(() => data.value), track(() => handle.value)];
    if (!d || !h) return;
    const center = d.projects.find((p) => p.id === viewProject.value);
    h.setData({ subjects: d.subjects, links: d.links, projects: d.projects, centerProject: center ? { id: center.id, name: center.name } : null });
    if (selected.value && !d.subjects.some((s) => s.id === selected.value)) selected.value = null;
  });

  // What the pressed subject holds.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const id = track(() => selected.value);
    const d = track(() => data.value);
    const h = track(() => handle.value);
    h?.select(id, true);
    allShown.value = false;
    naming.value = false;
    const subject = d?.subjects.find((s) => s.id === id);
    if (!subject) return void (held.value = []);
    const { invoke } = await import('@tauri-apps/api/core');
    const all = await invoke<KeptItem[]>('memory_items').catch(() => [] as KeptItem[]);
    held.value = all
      .filter((i) => LIVE.includes(i.item.state) && subject.ids.includes(String((i.item as { subject?: string | null }).subject ?? '')))
      .sort((a, b) => b.asOf - a.asOf);
  });

  const setMode = $((m: 'map' | 'list') => {
    mode.value = m;
    if (m === 'map')
      setTimeout(() => {
        handle.value?.resize();
        handle.value?.fit(300);
      }, 30);
  });

  const saveName = $(async () => {
    const id = selected.value;
    const name = nameDraft.value.trim();
    if (!id || !name) return;
    try {
      await renameSubject(id, name);
      naming.value = false;
      await load();
    } catch (e) {
      failed.value = String(e);
    }
  });

  const d = data.value;
  const subjects = d?.subjects ?? [];
  const subject = subjects.find((s) => s.id === selected.value) ?? null;
  const items = subjects.reduce((n, s) => n + s.items, 0);
  const aiName = ais.value.find((a) => a.founding === viewAi.value)?.name;
  const projectName = d?.projects.find((p) => p.id === viewProject.value)?.name;
  const who = aiName ? `What ${aiName} can use` : 'What your AIs can use';
  const sentence = `${who}${projectName ? ` in ${projectName}` : ''}: ${countWords(subjects.length, 'subject', 'subjects')}, ${countWords(items, 'item', 'items')}.`;
  const together = subject && d ? d.links.filter((l) => l.a === subject.id || l.b === subject.id).map((l) => subjects.find((s) => s.id === (l.a === subject.id ? l.b : l.a))?.name).filter(Boolean) : [];

  const pill = (on: boolean) =>
    `px-3 py-1 rounded-full text-xs font-medium transition-colors ${on ? 'bg-[var(--bg-button-primary)] text-[var(--text-button-primary)]' : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}`;
  const group = 'inline-flex flex-wrap gap-0.5 p-0.5 rounded-full border border-[var(--border-subtle)]';
  const tag = 'text-xs px-2.5 py-0.5 rounded-full bg-[var(--bg-main)] text-[var(--text-secondary)]';
  const zoomButton = 'w-8 h-8 rounded-[10px] border border-[var(--border-subtle)] bg-[var(--bg-dropdown)] text-[var(--text-primary)] leading-none';

  return (
    <section data-testid="memory-map" data-subjects={subjects.length}>
      {/* Whose view is chosen with the faces above. The row of projects stays hidden until there is one. */}
      {(d?.projects.length ?? 0) > 0 && (
        <div class="flex flex-col gap-2 mb-3">
          {(
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-xs text-[var(--text-secondary)]">Show</span>
              <div class={group}>
                <button type="button" class={pill(!viewProject.value)} data-testid="map-view-project" data-project="" onClick$={() => ((viewProject.value = ''), load())}>
                  Everywhere
                </button>
                {d!.projects.map((p) => (
                  <button key={p.id} type="button" class={pill(viewProject.value === p.id)} data-testid="map-view-project" data-project={p.id} onClick$={() => ((viewProject.value = p.id), load())}>
                    {p.name}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      <div class="flex flex-wrap items-start justify-between gap-3 mb-3">
        <p class="text-sm text-[var(--text-secondary)] max-w-2xl">
          What memory has kept, by subject. Your AIs can also search all your conversations.{' '}
          <span class="font-semibold text-[var(--text-primary)]" data-testid="map-sentence">
            {d ? sentence : ''}
          </span>
        </p>
        <div class={group}>
          <button type="button" class={pill(mode.value === 'map')} data-testid="map-mode-map" onClick$={() => setMode('map')}>
            Map
          </button>
          <button type="button" class={pill(mode.value === 'list')} data-testid="map-mode-list" onClick$={() => setMode('list')}>
            List
          </button>
        </div>
      </div>

      {failed.value && <p class="text-sm text-[var(--text-secondary)] mb-3">The map could not be read just now. {failed.value}</p>}

      <div class="grid gap-4 md:grid-cols-[minmax(0,1fr)_280px]">
        <div class={mode.value === 'map' ? 'min-w-0' : 'hidden'}>
          <div class="relative rounded-2xl border border-[var(--border-subtle)] overflow-hidden bg-[var(--bg-main)]" style={{ height: 'min(64vh, 560px)', minHeight: '360px' }}>
            <div ref={stage} class="absolute inset-0" data-testid="memory-map-stage" />
            {d && subjects.length <= 3 && (
              <div class="absolute inset-x-0 bottom-4 px-4 text-center text-sm text-[var(--text-secondary)] pointer-events-none" data-testid="map-grow-line">
                Your map grows as memory learns more.
              </div>
            )}
            <div class="absolute right-3 top-3 flex flex-col gap-1.5">
              <button type="button" class={zoomButton} aria-label="Zoom in" data-testid="map-zoom-in" onClick$={() => handle.value?.zoomBy(1.4)}>
                +
              </button>
              <button type="button" class={zoomButton} aria-label="Zoom out" data-testid="map-zoom-out" onClick$={() => handle.value?.zoomBy(1 / 1.4)}>
                &minus;
              </button>
              <button type="button" class={`${zoomButton} text-[11px] font-semibold`} aria-label="Fit the whole map" data-testid="map-zoom-fit" onClick$={() => handle.value?.fit(500)}>
                Fit
              </button>
            </div>
          </div>
        </div>

        {mode.value === 'list' && (
          <div class="min-w-0 rounded-2xl border border-[var(--border-subtle)] overflow-hidden self-start" data-testid="map-list">
            {subjects.map((s) => (
              <button
                key={s.id}
                type="button"
                data-testid="map-subject-row"
                data-id={s.id}
                onClick$={() => (selected.value = s.id)}
                class={`w-full flex flex-wrap justify-between gap-x-4 gap-y-1 px-4 py-3 text-left border-b border-[var(--border-subtle)] last:border-b-0 hover:bg-[var(--bg-card)] ${selected.value === s.id ? 'bg-[var(--bg-card)]' : ''}`}
              >
                <span class="font-medium text-[var(--text-primary)]">{s.name}</span>
                <span class="text-sm text-[var(--text-secondary)]">
                  {countWords(s.items, 'item', 'items')} · {audienceWords(s.audience)}
                  {s.lastCameUp ? ` · last came up ${ago(s.lastCameUp)}` : ''}
                </span>
              </button>
            ))}
          </div>
        )}

        <aside class="min-w-0 self-start rounded-2xl bg-[var(--bg-card)] p-4 flex flex-col gap-3" data-testid="map-panel" data-subject={subject?.id ?? ''}>
          {!subject ? (
            <>
              <h3 class="text-lg font-varela text-[var(--text-primary)]">Your map</h3>
              <p class="text-sm text-[var(--text-secondary)]">Each circle is a subject. Bigger means more is kept. A line joins subjects that come up together.</p>
              <p class="text-sm text-[var(--text-secondary)]">Scroll to zoom, drag to move, press a subject to see what it holds.</p>
              <div class="flex flex-col gap-1.5 text-xs text-[var(--text-secondary)]" data-testid="map-legend">
                <div class="flex items-center gap-2">
                  <span class="w-4 h-4 rounded-full border-[1.5px] border-solid border-[var(--text-secondary)]" />
                  For all your AIs
                </div>
                <div class="flex items-center gap-2">
                  <span class="w-4 h-4 rounded-full border-[1.5px] border-dashed border-[var(--text-secondary)]" />
                  Only for one AI
                </div>
                <div class="flex items-center gap-2">
                  <span class="w-4 h-4 rounded-full border-[1.5px] border-dotted border-[var(--text-secondary)]" />
                  For one project
                </div>
                {d?.projects.map((p) => (
                  <div key={p.id} class="flex items-center gap-2">
                    <span class="w-2.5 h-2.5 mx-[3px] rounded-full" style={{ background: p.color == null ? 'var(--map-quiet)' : `var(--map-project-${p.color + 1})` }} />
                    {p.name}
                  </div>
                ))}
                <div class="flex items-center gap-2">
                  <span class="w-2.5 h-2.5 mx-[3px] rounded-full" style={{ background: 'var(--map-none)' }} />
                  Not in a project
                </div>
              </div>
              {(d?.unplaced ?? 0) > 0 && <p class="text-xs text-[var(--text-muted)]">{countWords(d!.unplaced, 'kept item is', 'kept items are')} not in a subject yet.</p>}
            </>
          ) : (
            <>
              {naming.value ? (
                <div class="flex flex-col gap-2">
                  <input
                    type="text"
                    class="w-full px-3 py-2 rounded-lg bg-[var(--bg-input)] border border-[var(--border-input)] text-[var(--text-primary)] text-sm"
                    data-testid="map-name-input"
                    value={nameDraft.value}
                    maxLength={80}
                    onInput$={(_, el) => (nameDraft.value = el.value)}
                    onKeyDown$={(e) => e.key === 'Enter' && saveName()}
                  />
                  <div class="flex justify-end gap-2">
                    <LiquidMetalButton variant="secondary" class="px-3 py-1.5 text-sm" onClick$={() => (naming.value = false)}>
                      Cancel
                    </LiquidMetalButton>
                    <LiquidMetalButton class="px-3 py-1.5 text-sm" testId="map-name-save" onClick$={saveName}>
                      Save
                    </LiquidMetalButton>
                  </div>
                </div>
              ) : (
                <div>
                  <h3 class="text-lg font-varela text-[var(--text-primary)] break-words" data-testid="map-panel-name">
                    {subject.name}
                  </h3>
                  {subject.kind !== 'about_you' && (
                    <p class="text-xs text-[var(--text-muted)]">
                      {subject.nameAccepted ? '' : subject.kind === 'conversation' ? 'Named from the conversation and its dates. ' : ''}
                      <button
                        type="button"
                        class="text-[var(--text-link)] hover:underline"
                        data-testid="map-rename"
                        onClick$={() => {
                          nameDraft.value = subject.name;
                          naming.value = true;
                        }}
                      >
                        {subject.nameAccepted ? 'Rename' : 'Name it'}
                      </button>
                    </p>
                  )}
                </div>
              )}
              <div class="flex flex-wrap gap-1.5">
                <span class={tag} data-testid="map-panel-count">
                  {countWords(subject.items, 'item', 'items')}
                </span>
                {subject.open > 0 && <span class={tag}>{subject.open} open</span>}
                <span class={tag}>{audienceWords(subject.audience)}</span>
              </div>
              <p class="text-sm text-[var(--text-secondary)]">
                {subject.lastCameUp ? `Last came up ${ago(subject.lastCameUp)}.` : 'Has not come up in an answer on this computer yet.'}
                {together.length > 0 ? ` Comes up together with ${together.join(', ')}.` : ''}
              </p>
              <SubjectItems subject={subject} held={held.value} all={allShown.value} onAll$={() => (allShown.value = true)} />
            </>
          )}
        </aside>
      </div>
    </section>
  );
});

/** What a subject holds, newest first. */
const SubjectItems = component$<{ subject: MapSubject; held: KeptItem[]; all: boolean; onAll$: () => void }>((props) => {
  const shown = props.all ? props.held : props.held.slice(0, PANEL_ITEMS);
  if (props.subject.items === 0) {
    return (
      <p class="text-sm text-[var(--text-secondary)]" data-testid="map-panel-empty">
        {props.subject.kind === 'about_you' ? 'Nothing is kept about you yet. Say "remember that" in any chat, or press Remember under a reply.' : 'Nothing is kept here now.'}
      </p>
    );
  }
  return (
    <div class="flex flex-col gap-2" data-testid="map-panel-items">
      {shown.map((i) => (
        <div key={i.id} class="rounded-lg bg-[var(--bg-main)] px-3 py-2" data-testid="map-panel-item">
          <div class="text-sm text-[var(--text-primary)] break-words">{i.text}</div>
          <div class="text-xs text-[var(--text-muted)]">
            {dayOf(i.asOf)}
            {i.item.state === 'open' ? ' · open' : ''}
          </div>
        </div>
      ))}
      {!props.all && props.held.length > shown.length && (
        <button type="button" class="self-start text-xs text-[var(--text-link)] hover:underline" data-testid="map-panel-all" onClick$={props.onAll$}>
          Show all {props.held.length}
        </button>
      )}
    </div>
  );
});
