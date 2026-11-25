/**
 * Under a reply that was made off this computer: what memory sent with
 * the question, the page that explains it, and the AI's switch. Nothing
 * here is asked before a message is sent.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { firstSentWords, sentWords, type Sent, sentAnything } from "../../utils/givenLine";
import {
  getLocalCustomAis,
  updateLocalCustomAi,
} from "../../utils/localAiStorage";
import { hadOlderMemory, updateReport } from "../../utils/memoryUpdate";

/** Sent when the switch is changed from a line, so the open chat uses it
 *  with the next message. */
export const MEMORY_SHARED_EVENT = "yoai-memory-shared";
export interface MemorySharedDetail {
  aiId: string;
  on: boolean;
}

/** The AIs whose one line is on the page now. */
const claimed = new Set<string>();

export async function setMemoryShared(
  aiId: string,
  on: boolean,
): Promise<void> {
  await updateLocalCustomAi(aiId, { memoryOnline: on });
  window.dispatchEvent(
    new CustomEvent<MemorySharedDetail>(MEMORY_SHARED_EVENT, {
      detail: { aiId, on },
    }),
  );
}

export const SentLine = component$<{
  sent?: Sent;
  servedBy: string;
  aiId: string;
  aiName: string;
  off?: boolean;
  isLast?: boolean;
}>((props) => {
  const nav = useNavigate();
  const changed = useSignal<"" | "off" | "on" | "failed">("");

  // The one line, once for this AI: under the newest reply, the first
  // time memory went with a message. It is marked seen as it is shown, so
  // it stays under this reply and comes back under no other.
  const first = useSignal<"" | "new" | "changed">("");
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    if (
      !props.isLast ||
      props.off ||
      !props.sent ||
      !sentAnything(props.sent)
    )
      return;
    if (claimed.has(props.aiId)) return;
    const ai = (await getLocalCustomAis().catch(() => [])).find(
      (a) => a.id === props.aiId,
    );
    if (!ai || ai.memoryOnlineLineSeen || claimed.has(props.aiId)) return;
    claimed.add(props.aiId);
    const report = await updateReport();
    const wasHere =
      hadOlderMemory(report) && (ai.updatedAt ?? 0) < (report?.doneAt ?? 0);
    first.value = wasHere ? "changed" : "new";
    await updateLocalCustomAi(props.aiId, { memoryOnlineLineSeen: true }).catch(
      () => undefined,
    );
  });

  const set = $(async (on: boolean) => {
    try {
      await setMemoryShared(props.aiId, on);
      changed.value = on ? "on" : "off";
    } catch {
      changed.value = "failed";
    }
  });

  const link = "underline";
  return (
    <>
      {first.value && (
        <div
          class="mb-1 text-[var(--text-primary)]"
          data-testid="sent-first"
          data-form={first.value}
        >
          {firstSentWords(
            props.aiName,
            props.servedBy,
            first.value === "changed",
          )}
        </div>
      )}
      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1"
        data-testid="given-sent"
      >
        {props.sent && <span>{sentWords(props.sent, props.servedBy)}</span>}
        <button
          type="button"
          class={link}
          data-testid="sent-what"
          onClick$={() => nav("/memory-sent/")}
        >
          What is sent
        </button>
        {changed.value === "" && !props.off && (
          <button
            type="button"
            class={link}
            data-testid="sent-turn-off"
            onClick$={() => set(false)}
          >
            Turn off for {props.aiName}
          </button>
        )}
        {changed.value === "" && props.off && (
          <button
            type="button"
            class={link}
            data-testid="sent-turn-on"
            onClick$={() => set(true)}
          >
            Turn on
          </button>
        )}
        {changed.value === "off" && (
          <span data-testid="sent-changed">
            Off for {props.aiName}, from your next message.
          </span>
        )}
        {changed.value === "on" && (
          <span data-testid="sent-changed">
            On for {props.aiName}, from your next message.
          </span>
        )}
        {changed.value === "failed" && (
          <span data-testid="sent-changed">Could not change it just now.</span>
        )}
      </div>
    </>
  );
});
