/**
 * Offers you declined: instructions you gave more than once and answered
 * No when asked to keep them as a rule. They are not offered again unless
 * you say so here. Shown only when there is one.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";

export const DeclinedOffers = component$(() => {
  const texts = useSignal<string[]>([]);
  const busy = useSignal("");

  const load = $(async () => {
    texts.value = await invoke<string[]>("memory_rule_offers_declined").catch(() => []);
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    await load();
  });

  const again = $(async (text: string) => {
    busy.value = text;
    await invoke("memory_rule_offer_forget", { text }).catch(() => undefined);
    await load();
    busy.value = "";
  });

  if (texts.value.length === 0) return null;
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="declined-offers">
      <h3 class="text-lg font-semibold text-[var(--text-primary)]">Offers you declined</h3>
      <p class="mt-1 text-sm text-[var(--text-secondary)]">
        You asked for these more than once and chose not to keep them as rules. They are not offered again.
      </p>
      <ul class="mt-4 divide-y divide-[var(--border-subtle)]">
        {texts.value.map((t) => (
          <li key={t} class="py-3 flex items-start gap-3" data-testid="declined-offer">
            <div class="min-w-0 flex-1 text-sm text-[var(--text-primary)] break-words">{t}</div>
            <button
              type="button"
              data-testid="declined-offer-again"
              disabled={busy.value !== ""}
              onClick$={() => again(t)}
              class="shrink-0 text-xs font-medium text-[var(--text-link)] hover:underline disabled:opacity-50"
            >
              Offer it again
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
});
