/**
 * Who can use one conversation, in the conversation's record: one quiet
 * line, and the choices one press away. A change takes effect with the next
 * question; nothing is read again.
 */
import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import type { Audience } from '../../utils/memoryKeep';
import { type ConvUse, conversationUse, setAudience, setPrivate } from '../../utils/memoryUse';

interface Ai {
  founding: string;
  name: string;
}

/** Who may use it, as the end of "... can use this conversation." */
export function whoWords(a: Audience | undefined, ais: Ai[]): string {
  switch (a?.kind) {
    case 'ai':
      return `Only ${ais.find((x) => x.founding === a.ai)?.name ?? 'one AI'}`;
    case 'local_ais':
      return 'Only AIs on your devices';
    case 'project':
      return 'Only AIs working in its project';
    case 'none':
      return 'No AI';
    default:
      return 'All your AIs';
  }
}

export const ConversationUse = component$<{ startedAt: number }>((props) => {
  const use = useSignal<ConvUse | null>(null);
  const ais = useSignal<Ai[]>([]);
  const choosing = useSignal(false);
  const busy = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const at = track(() => props.startedAt);
    use.value = await conversationUse(at);
    const { getLocalCustomAis } = await import('../../utils/localAiStorage');
    ais.value = (await getLocalCustomAis().catch(() => []))
      .filter((a) => a.status !== 'archived')
      .map((a) => ({ founding: a.foundingKey || a.agentPubKey || '', name: a.name }))
      .filter((a) => a.founding);
  });

  const choose = $(async (audience: Audience) => {
    busy.value = true;
    try {
      use.value = await setAudience(props.startedAt, audience);
      choosing.value = false;
    } finally {
      busy.value = false;
    }
  });
  const useAgain = $(async () => {
    busy.value = true;
    try {
      use.value = await setPrivate(props.startedAt, false);
    } finally {
      busy.value = false;
    }
  });

  const u = use.value;
  if (!u) return null;
  const act = 'text-xs text-[var(--text-link)] hover:underline disabled:opacity-50';
  return (
    <div class="text-xs text-[var(--text-muted)]" data-testid="conversation-use" data-audience={u.private ? 'private' : (u.audience?.kind ?? 'all')}>
      {u.private ? (
        <>
          Kept out of memory. It is never searched and nothing is kept from it.{' '}
          <button type="button" class={act} data-testid="conversation-use-again" disabled={busy.value} onClick$={useAgain}>
            Use it in memory
          </button>
        </>
      ) : choosing.value ? (
        <div class="flex flex-col items-start gap-1" data-testid="conversation-use-list">
          <span class="text-[var(--text-secondary)]">Who can use this conversation</span>
          <button type="button" class={act} data-testid="conversation-use-to" data-to="all" disabled={busy.value} onClick$={() => choose({ kind: 'all' })}>
            All your AIs
          </button>
          {ais.value.map((a) => (
            <button key={a.founding} type="button" class={act} data-testid="conversation-use-to" data-to={a.founding} disabled={busy.value} onClick$={() => choose({ kind: 'ai', ai: a.founding })}>
              Only {a.name}
            </button>
          ))}
          <button type="button" class={act} data-testid="conversation-use-to" data-to="local_ais" disabled={busy.value} onClick$={() => choose({ kind: 'local_ais' })}>
            Only AIs on your devices
          </button>
          <button type="button" class="hover:underline" onClick$={() => (choosing.value = false)}>
            Cancel
          </button>
        </div>
      ) : (
        <>
          <span data-testid="conversation-use-words">{whoWords(u.audience, ais.value)} can use this conversation.</span>{' '}
          <button type="button" class={act} data-testid="conversation-use-change" onClick$={() => (choosing.value = true)}>
            Change
          </button>
        </>
      )}
    </div>
  );
});
