import { component$, useSignal, useTask$, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { LuLoader2, LuMessageSquare, LuSearch } from '@qwikest/icons/lucide';
import { snippetParts, type SearchHit, type SearchProgress } from '../utils/transcriptSearch';

/** Records timestamps are microseconds (the same rule as the page's rows). */
function when(timestamp: number): string {
  const ms = timestamp > 1e15 ? timestamp / 1000 : timestamp;
  return new Date(ms).toLocaleString();
}

export interface ConversationSearchProps {
  /** The AI whose conversations are searched. */
  aiId: string;
  /** Its current agent key - what "Read them now" lists by. */
  agentKey: string;
  /** What the person typed in the conversations filter box. */
  query: string;
  onOpen$: QRL<(hit: SearchHit) => void>;
}

/**
 * The words half of the conversations filter: the title filter narrows the
 * list above; this searches INSIDE the conversations and lists the ones
 * whose messages match, best first, with the matching stretch. The first
 * time, the conversations are read for search (once; every turn recorded
 * after that joins the index as it happens).
 */
export const ConversationSearch = component$<ConversationSearchProps>((props) => {
  const hits = useSignal<SearchHit[]>([]);
  const needsRead = useSignal(false);
  const searching = useSignal(false);
  const progress = useSignal<SearchProgress | null>(null);
  const error = useSignal('');
  const warming = useSignal(false);

  const search = $(async (aiId: string, agentKey: string, query: string) => {
    if (query.trim().length < 3 || !aiId) {
      hits.value = [];
      needsRead.value = false;
      return;
    }
    searching.value = true;
    error.value = '';
    try {
      const { transcriptSearch } = await import('../utils/transcriptSearch');
      const { WARMUP_POLL_MS } = await import('../utils/recordsWarmup');
      // Just after launch the records take a moment: the same wait the
      // page shows, then the search runs on its own.
      const deadline = Date.now() + 5 * 60_000;
      let a = await transcriptSearch(aiId, agentKey, query, 30);
      while (a.warming && Date.now() < deadline) {
        warming.value = true;
        await new Promise((r) => setTimeout(r, WARMUP_POLL_MS));
        a = await transcriptSearch(aiId, agentKey, query, 30);
      }
      warming.value = false;
      hits.value = a.hits;
      needsRead.value = a.needs_read;
      if (a.building && !progress.value) progress.value = { done: 0, total: 0, finished: false, cancelled: false };
    } catch (e) {
      const text = String(e);
      // Not an error: the records are not answering yet - say so, in the app's words.
      error.value = text.includes('still starting') ? '' : text;
      warming.value = text.includes('still starting');
    } finally {
      searching.value = false;
    }
  });

  // Debounced: typing narrows as it goes without a search per keystroke.
  useTask$(({ track, cleanup }) => {
    const q = track(() => props.query);
    const id = track(() => props.aiId);
    const k = track(() => props.agentKey);
    const t = setTimeout(() => void search(id, k, q), 250);
    cleanup(() => clearTimeout(t));
  });

  const readNow = $(async () => {
    error.value = '';
    progress.value = { done: 0, total: 0, finished: false, cancelled: false };
    try {
      const { transcriptSearchBuild } = await import('../utils/transcriptSearch');
      await transcriptSearchBuild(props.agentKey);
    } catch (e) {
      error.value = String(e);
      progress.value = null;
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const { listen } = await import('@tauri-apps/api/event');
    const un = await listen<SearchProgress>('transcript-search-progress', (e) => {
      progress.value = e.payload.finished ? null : e.payload;
      if (e.payload.finished) void search(props.aiId, props.agentKey, props.query);
    });
    cleanup(() => un());
  });

  if (props.query.trim().length < 3) return null;

  return (
    <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-3 space-y-2">
      <div class="flex items-center gap-2 text-xs text-[var(--text-muted)]">
        {searching.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuSearch class="w-3.5 h-3.5" />}
        <span>
          In the conversations
          {hits.value.length ? ` - ${hits.value.length}${hits.value.length === 30 ? '+' : ''}` : ''}
        </span>
        {!needsRead.value && !progress.value && !warming.value && (
          <button type="button" class="ml-auto text-[var(--text-link)] hover:underline" onClick$={readNow} title="Ask the records what changed: new and continued conversations are read, deleted ones leave">
            Refresh
          </button>
        )}
      </div>
      {warming.value && (
        <div class="flex items-center gap-3 py-2">
          <div class="w-5 h-5 border-2 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
          <div>
            <p class="text-sm text-[var(--text-primary)]">Your records are warming up</p>
            <p class="text-xs text-[var(--text-secondary)]">Just after launch, your conversations take a moment to be ready - the search runs as soon as they are.</p>
          </div>
        </div>
      )}
      {progress.value && (
        <p class="text-xs text-[var(--text-secondary)]">
          Reading your conversations for search
          {progress.value.total ? ` - ${progress.value.done} of ${progress.value.total}` : '...'}
          {' '}
          <button
            type="button"
            class="text-[var(--text-link)] hover:underline"
            onClick$={async () => { const m = await import('../utils/transcriptSearch'); await m.transcriptSearchCancel(); }}
          >
            Stop
          </button>
        </p>
      )}
      {needsRead.value && !progress.value && (
        <p class="text-xs text-[var(--text-secondary)]">
          The words inside your conversations have not been read for search yet.{' '}
          <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={readNow}>
            Read them now
          </button>
          {' '}- once, on this computer; new messages join as you go.
        </p>
      )}
      {error.value && <p class="text-xs text-red-500">{error.value}</p>}
      {!needsRead.value && !searching.value && !progress.value && !warming.value && hits.value.length === 0 && (
        <p class="text-xs text-[var(--text-muted)]">No messages match.</p>
      )}
      {hits.value.map((h) => (
        <button
          key={`${h.hash}:${h.seq}`}
          type="button"
          onClick$={() => props.onOpen$(h)}
          class="block w-full text-left rounded-lg px-3 py-2 hover:bg-[var(--bg-main)] transition-colors"
        >
          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-[var(--text-muted)]">
            <LuMessageSquare class="w-3.5 h-3.5 shrink-0" />
            <span class="font-medium text-[var(--text-primary)] truncate max-w-[24rem]">{h.title || 'Untitled conversation'}</span>
            <span>{h.role === 'user' ? 'you' : h.ai_name}</span>
            <span>·</span>
            <span>{when(h.at)}</span>
            {h.source?.startsWith('import:') && <span class="rounded-full border border-[var(--border-subtle)] px-1.5">imported</span>}
            {h.more > 0 && <span>· {h.more} more {h.more === 1 ? 'match' : 'matches'}</span>}
          </div>
          <p class="mt-0.5 text-sm text-[var(--text-secondary)] line-clamp-2">
            {snippetParts(h.snippet).map(([text, hit], i) =>
              hit ? (
                <mark key={i} class="rounded bg-[var(--text-link)]/20 text-[var(--text-primary)] px-0.5">{text}</mark>
              ) : (
                <span key={i}>{text}</span>
              ),
            )}
          </p>
        </button>
      ))}
    </div>
  );
});

export default ConversationSearch;
