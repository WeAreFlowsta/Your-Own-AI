/**
 * "You have two AIs named Teresa ... Are they the same AI?" One card on the
 * home page for two starters that look like one AI and have no answer yet
 * (utils/lookalikes.ts). Nothing is joined until the person says so; "Keep
 * both" is remembered on every device. AIs the person made under one name
 * get only the quiet line on Your AIs, never this card.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { Callout } from "./Callout";
import LiquidMetalButton from "./LiquidMetalButton";
import { readPairing, mergePair, keepPairApart, pairQuestion, type Lookalike } from "../utils/lookalikes";

export const LookalikeCard = component$(() => {
  const pair = useSignal<Lookalike | null>(null);
  const busy = useSignal(false);
  const error = useSignal("");

  const refresh = $(async () => {
    const all = (await readPairing()).lookalikes;
    pair.value = all.find((p) => p.kind === "starter") ?? null;
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refresh();
    try {
      const { listen } = await import("@tauri-apps/api/event");
      const un = await listen("ai-configs-changed", () => void refresh());
      cleanup(() => un());
    } catch {
      /* not inside Tauri */
    }
  });

  const answer = $(async (merge: boolean) => {
    const p = pair.value;
    if (!p || busy.value) return;
    busy.value = true;
    error.value = "";
    try {
      await (merge ? mergePair(p.a, p.b) : keepPairApart(p.a, p.b));
      await refresh();
    } catch (e) {
      error.value = String(e);
    } finally {
      busy.value = false;
    }
  });

  if (!pair.value) return null;
  return (
    <Callout intent="info" class="mt-4">
      <p data-testid="lookalike-question">{pairQuestion(pair.value)}</p>
      <p class="mt-1 text-xs text-[var(--text-muted)]">
        Merged, you see one {pair.value.name} with both sets of conversations. Nothing is deleted, and you can separate them again on Your AIs.
      </p>
      {error.value && <p class="mt-1 text-xs text-red-400">{error.value}</p>}
      <div class="mt-3 flex flex-col-reverse sm:flex-row sm:justify-end gap-2">
        <LiquidMetalButton variant="secondary" testId="lookalike-keep" disabled={busy.value} onClick$={() => answer(false)} class="px-4 py-2 text-sm">
          Keep both
        </LiquidMetalButton>
        <LiquidMetalButton testId="lookalike-merge" disabled={busy.value} onClick$={() => answer(true)} class="px-4 py-2 text-sm">
          Merge them
        </LiquidMetalButton>
      </div>
    </Callout>
  );
});
