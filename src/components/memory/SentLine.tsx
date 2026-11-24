/**
 * Under a reply that was made off this computer: what memory sent with
 * the question, the page that explains it, and the AI's switch. Nothing
 * here is asked before a message is sent.
 */
import { component$, useSignal, $ } from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { sentWords, type Sent } from "../../utils/givenLine";
import { updateLocalCustomAi } from "../../utils/localAiStorage";

/** Sent when the switch is changed from a line, so the open chat uses it
 *  with the next message. */
export const MEMORY_SHARED_EVENT = "yoai-memory-shared";
export interface MemorySharedDetail {
  aiId: string;
  on: boolean;
}

export async function setMemoryShared(aiId: string, on: boolean): Promise<void> {
  await updateLocalCustomAi(aiId, { memoryOnline: on });
  window.dispatchEvent(new CustomEvent<MemorySharedDetail>(MEMORY_SHARED_EVENT, { detail: { aiId, on } }));
}

export const SentLine = component$<{ sent?: Sent; servedBy: string; aiId: string; aiName: string; off?: boolean }>((props) => {
  const nav = useNavigate();
  const changed = useSignal<"" | "off" | "on" | "failed">("");

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
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="given-sent">
      {props.sent && <span>{sentWords(props.sent, props.servedBy)}</span>}
      <button type="button" class={link} data-testid="sent-what" onClick$={() => nav("/memory-sent/")}>
        What is sent
      </button>
      {changed.value === "" && !props.off && (
        <button type="button" class={link} data-testid="sent-turn-off" onClick$={() => set(false)}>
          Turn off for {props.aiName}
        </button>
      )}
      {changed.value === "" && props.off && (
        <button type="button" class={link} data-testid="sent-turn-on" onClick$={() => set(true)}>
          Turn on
        </button>
      )}
      {changed.value === "off" && <span data-testid="sent-changed">Off for {props.aiName}, from your next message.</span>}
      {changed.value === "on" && <span data-testid="sent-changed">On for {props.aiName}, from your next message.</span>}
      {changed.value === "failed" && <span data-testid="sent-changed">Could not change it just now.</span>}
    </div>
  );
});
