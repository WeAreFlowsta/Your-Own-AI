/**
 * Not used: conversations the person kept out of memory, or left messages
 * out of, each with "Use again". Shown only when there is one.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { CONV_USE_EVENT, conversationsNotUsed, notUsedWords, useAgain, type ConvUse } from "../../utils/memoryUse";
import { getConversationsCached } from "../../utils/holochainTranscripts";
import { getLocalCustomAis } from "../../utils/localAiStorage";

export const NotUsedList = component$(() => {
  const rows = useSignal<ConvUse[]>([]);
  const titles = useSignal<Record<string, string>>({});
  const busy = useSignal(0);

  const load = $(async () => {
    rows.value = await conversationsNotUsed();
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load();
    window.addEventListener(CONV_USE_EVENT, load);
    cleanup(() => window.removeEventListener(CONV_USE_EVENT, load));
    if (rows.value.length === 0) return;
    const byStart: Record<string, string> = {};
    for (const ai of await getLocalCustomAis().catch(() => [])) {
      if (!ai.agentPubKey) continue;
      for (const c of await getConversationsCached(ai.agentPubKey)) byStart[String(c.started_at)] = c.title || "";
    }
    titles.value = byStart;
  });

  const again = $(async (u: ConvUse) => {
    busy.value = u.started_at;
    await useAgain(u).catch(() => undefined);
    await load();
    busy.value = 0;
  });

  if (rows.value.length === 0) return null;
  const day = (us: number) => new Date(us > 1e15 ? us / 1000 : us).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" });
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="not-used-list">
      <h3 class="text-lg font-semibold text-[var(--text-primary)]">Not used</h3>
      <p class="mt-1 text-sm text-[var(--text-secondary)]">Conversations you kept out of memory, in whole or in part. They stay in your conversations.</p>
      <ul class="mt-4 divide-y divide-[var(--border-subtle)]">
        {rows.value.map((u) => (
          <li key={u.started_at} class="py-3 flex items-start gap-3" data-testid="not-used-row" data-private={u.private ? "1" : "0"}>
            <div class="min-w-0 flex-1">
              <div class="text-sm text-[var(--text-primary)] break-words">{titles.value[String(u.started_at)] || `Conversation of ${day(u.started_at)}`}</div>
              <div class="mt-0.5 text-xs text-[var(--text-muted)]">{notUsedWords(u)}</div>
            </div>
            <button
              type="button"
              data-testid="not-used-again"
              disabled={busy.value !== 0}
              onClick$={() => again(u)}
              class="shrink-0 text-xs font-medium text-[var(--text-link)] hover:underline disabled:opacity-50"
            >
              Use again
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
});
