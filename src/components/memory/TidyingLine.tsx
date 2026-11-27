import { useMoreMemoryControls } from '../../utils/memoryMore';
import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';

interface Marks {
  part: 'judge' | 'gate' | 'done';
  examples: number;
  caught: number;
  of_yes: number;
  wrong_yes: number;
  of_no: number;
  right: number;
  passed: boolean;
}
interface Run {
  parts?: Marks[];
}
interface TidyStatus {
  state: 'on' | 'off' | 'paused' | 'checking' | 'not-passed' | 'no-local-model' | 'elsewhere' | '';
  model: string | null;
  mayKeep: boolean;
  result: { may_tidy: boolean; may_keep: boolean; runs?: Run[] } | null;
  done: number;
  total: number;
  queued: number;
}

/** A model file's name as a person reads it. */
const modelName = (file: string | null) => (file ?? '').replace(/\.gguf$/i, '').replace(/[-_.]?Q\d[\w]*$/i, '') || 'The model';

/** The check's marks in words, from the run that went worst for each part. */
function marks(result: TidyStatus['result']): string {
  const worst = (part: Marks['part'], bad: (m: Marks) => number) => {
    const all = (result?.runs ?? []).map((r) => r.parts?.find((p) => p.part === part)).filter((p): p is Marks => !!p);
    return all.length ? all.reduce((a, b) => (bad(b) > bad(a) ? b : a)) : null;
  };
  const judge = worst('judge', (m) => m.wrong_yes * 100 - m.caught);
  const gate = worst('gate', (m) => -m.right);
  const said = [
    judge
      ? `On "does this replace that" it caught ${judge.caught} of ${judge.of_yes} replacements and wrongly replaced ${judge.wrong_yes} of ${judge.of_no} that both stand. To pass it must catch 5 in 6 and may be wrong once.`
      : '',
    gate ? `On "was something settled" it got ${gate.right} of ${gate.examples} right. To pass it needs 4 in 5.` : '',
  ].filter(Boolean);
  return said.join(' ');
}

/**
 * The tidying line: whether memory replaces and closes things by itself
 * on this computer, why or why not, and the one action that fits.
 */
export const TidyingLine = component$(() => {
  const more = useMoreMemoryControls();
  const status = useSignal<TidyStatus | null>(null);
  const busy = useSignal(false);
  const showMarks = useSignal(false);

  const load = $(async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    status.value = await invoke<TidyStatus>('memory_tidy_status');
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load().catch(() => {});
    const t = setInterval(() => void load().catch(() => {}), 30_000);
    cleanup(() => clearInterval(t));
  });

  const act = $(async (cmd: 'off' | 'on' | 'again') => {
    if (busy.value) return;
    busy.value = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      if (cmd === 'again') await invoke('memory_tidy_start_again');
      else await invoke('memory_tidy_set', { off: cmd === 'off' });
      await load();
    } catch {
      /* the line stays as it was */
    }
    busy.value = false;
  });

  const s = status.value;
  if (!s || !s.state) return null;
  // At rest the line shows only when tidying stopped and waits for the
  // person. How it is going otherwise is one of the further controls.
  if (!more.value && s.state !== 'paused') return null;
  const name = modelName(s.model);
  const link = 'underline hover:text-[var(--text-primary)] disabled:opacity-50';
  const words: Record<string, string> = {
    on: `Tidying is on. ${name} passed the tidying check, so memory replaces and closes things by itself while the computer is idle.`,
    off: 'You turned tidying off. Memory finds things and keeps what you ask. It changes nothing by itself.',
    paused: 'Tidying is paused. Several of its changes were undone, so memory changes nothing by itself until you start it again.',
    checking: `Checking whether ${name} can keep memory up to date by itself. ${s.done} of ${s.total} examples done. It uses made-up examples, never your conversations. Finding things already works.`,
    'not-passed': `Tidying is off on this computer. No model here passed the tidying check, so memory does not replace or close things by itself. It still finds everything and keeps what you ask.${s.mayKeep ? ` ${name} still keeps what was settled. It does not replace or close things by itself.` : ''}`,
    'no-local-model': 'Tidying needs a model on this computer.',
    elsewhere: 'Tidying is on. Memory replaces and closes things by itself. Changes made on another of your devices show here too. Turning it off turns it off on all your devices.',
  };
  const checked = marks(s.result);

  return (
    <div class="mb-4 rounded-xl border border-[var(--border-subtle)] px-4 py-3 text-sm text-[var(--text-secondary)]" data-testid="tidying-line" data-state={s.state}>
      <span>{words[s.state] ?? ''}</span>
      <span class="ml-2 inline-flex items-center gap-3">
        {(s.state === 'on' || s.state === 'elsewhere') && (
          <button type="button" class={link} disabled={busy.value} onClick$={() => act('off')} data-testid="tidying-off">
            Turn off
          </button>
        )}
        {s.state === 'off' && (
          <button type="button" class={link} disabled={busy.value} onClick$={() => act('on')} data-testid="tidying-on">
            Turn on
          </button>
        )}
        {s.state === 'paused' && (
          <button type="button" class={link} disabled={busy.value} onClick$={() => act('again')} data-testid="tidying-again">
            Start again
          </button>
        )}
        {checked && (s.state === 'on' || s.state === 'not-passed') && (
          <button type="button" class={link} aria-expanded={showMarks.value} onClick$={() => (showMarks.value = !showMarks.value)} data-testid="tidying-marks-show">
            What the check is
          </button>
        )}
      </span>
      {showMarks.value && checked && (
        <div class="mt-2 text-[var(--text-muted)]" data-testid="tidying-marks">
          {checked}
        </div>
      )}
    </div>
  );
});
