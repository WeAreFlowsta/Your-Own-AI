import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import type { KeptItem } from '../../utils/memoryKeep';
import { dayOf } from '../../utils/memoryKeep';

interface Question {
  id: string;
  older: KeptItem;
  newer: KeptItem;
  now: string;
}

/** Sent when memory may have a new question (a tidying pass ended, a reply finished). */
export const MEMORY_QUESTIONS_EVENT = 'yoai-memory-questions';

const short = (s: string, n = 160) => (s.length > n ? `${s.slice(0, n).trimEnd()}…` : s);

/**
 * The question about something the person confirmed, when a later item
 * seems to change it. A line, never a gate: nothing is replaced until the
 * person answers, and the line can be left unanswered for good.
 */
export const MemoryQuestions = component$<{ limit?: number }>((props) => {
  const questions = useSignal<Question[]>([]);
  const busy = useSignal('');

  const load = $(async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    questions.value = await invoke<Question[]>('memory_questions');
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load().catch(() => {});
    const again = () => void load().catch(() => {});
    window.addEventListener(MEMORY_QUESTIONS_EVENT, again);
    const t = setInterval(again, 60_000);
    cleanup(() => {
      window.removeEventListener(MEMORY_QUESTIONS_EVENT, again);
      clearInterval(t);
    });
  });

  const answer = $(async (id: string, how: 'replace' | 'both' | 'ignore') => {
    if (busy.value) return;
    busy.value = id;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('memory_question_answer', { id, answer: how });
      await load();
    } catch {
      /* the question stays */
    }
    busy.value = '';
  });

  const shown = questions.value.slice(0, props.limit ?? questions.value.length);
  if (shown.length === 0) return null;
  const link = 'underline hover:text-[var(--text-primary)] disabled:opacity-50';

  return (
    <div class="space-y-2" data-testid="memory-questions">
      {shown.map((q) => {
        const confirmedAt = (q.older.item.confirmed as { at?: number } | null | undefined)?.at ?? q.older.asOf;
        return (
          <div key={q.id} class="text-xs leading-relaxed text-[var(--text-muted)]" data-testid="memory-question">
            <span>
              You confirmed "{short(q.older.text)}" on {dayOf(confirmedAt)}. Since then, on {dayOf(q.newer.asOf)}: "{short(q.newer.text)}"
            </span>
            <span class="ml-2 inline-flex items-center gap-3">
              <button type="button" class={link} disabled={busy.value === q.id} onClick$={() => answer(q.id, 'replace')} data-testid="question-replace">
                Replace it
              </button>
              <button type="button" class={link} disabled={busy.value === q.id} onClick$={() => answer(q.id, 'both')} data-testid="question-both">
                Both are true
              </button>
              <button type="button" class={link} disabled={busy.value === q.id} onClick$={() => answer(q.id, 'ignore')} data-testid="question-ignore">
                Ignore
              </button>
            </span>
          </div>
        );
      })}
    </div>
  );
});
