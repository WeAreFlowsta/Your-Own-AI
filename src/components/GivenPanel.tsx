/**
 * What an AI was given by memory for one reply: the passages from the
 * person's conversations, in the order they were given, read again from
 * the reply's record (pointers, never a second copy of the text) and
 * checked against the fingerprint kept with it.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import LiquidMetalButton from "./LiquidMetalButton";
import { dayWords } from "../utils/givenLine";

interface Passage {
  state: "shown" | "deleted" | "not-here";
  day: string;
  title: string;
  aiName: string | null;
  conversationId: string;
  seq: number;
  lines: [boolean, string][];
  quoted: boolean;
}

/** A found passage that was left out because memory holds something newer. */
interface LeftOut extends Omit<Passage, "state"> {
  state: "left-out";
  /** The replaced item it was left out for. */
  item: string;
  replacedOn: string | null;
  stillReplaced: boolean;
}

interface Panel {
  checked: "exact" | "some-gone" | "differs" | "no-record";
  passages: Passage[];
  deleted: number;
  notHere: number;
  lines: string[];
  instruction: string | null;
  leftOut: LeftOut[];
}

interface Props {
  /** The reply's record of what it was given. */
  given: Record<string, unknown>;
  /** The AI's name, as the person knows it. */
  ai: string;
}

const FIRST = 3;

export default component$<Props>((props) => {
  const panel = useSignal<Panel | null>(null);
  const failed = useSignal("");
  const all = useSignal(false);
  const exact = useSignal(false);
  // Items the person has just said are not replaced, by id.
  const putBack = useSignal<string[]>([]);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      panel.value = await invoke<Panel>("memory_given_texts", { given: props.given });
    } catch (e) {
      failed.value = String(e);
    }
  });

  const p = panel.value;
  const also = (props.given.also ?? {}) as { rules?: number };
  const total = p ? p.passages.length : 0;
  const shown = p ? (all.value ? p.passages : p.passages.slice(0, FIRST)) : [];

  return (
    <div class="px-4 pb-4 -mt-2 text-sm text-[var(--text-secondary)]" data-testid="given-panel">
      <div class="pt-4 space-y-4">
        {!p && !failed.value && <p class="text-xs text-[var(--text-muted)]">Reading what {props.ai} was given…</p>}
        {failed.value && <p class="text-xs text-[var(--text-muted)]">This could not be read just now. Your conversations are whole.</p>}
        {p && (
          <>
            <p class="text-xs" data-testid="given-checked" data-checked={p.checked}>
              {p.checked === "exact" && `This is exactly what ${props.ai} was given.`}
              {p.checked === "some-gone" &&
                `Some of this can no longer be shown.${p.deleted ? ` ${p.deleted} of the ${total} ${total === 1 ? "passage" : "passages"} came from a conversation you deleted.` : ""}${p.notHere ? ` ${p.notHere} ${p.notHere === 1 ? "is" : "are"} not on this computer yet.` : ""} The reply itself stays as it was.`}
              {p.checked === "differs" && "This could not be checked against the original. The conversations were read again since, and a passage no longer matches word for word. It is shown as it reads now."}
            </p>
            <ul class="space-y-3">
              {shown.map((x, i) => (
                <li key={`${x.conversationId}-${x.seq}-${i}`} data-testid="given-passage" data-state={x.state}>
                  <div class="flex items-baseline gap-2 text-xs text-[var(--text-muted)]">
                    <span>{dayWords(x.day)}</span>
                    {x.quoted && <span class="text-[var(--text-primary)]" data-testid="given-quoted">Quoted in the reply</span>}
                  </div>
                  {x.state === "shown" ? (
                    <>
                      <blockquote class="mt-1 pl-3 border-l-2 border-[var(--border-subtle)] space-y-1">
                        {x.lines.map(([byPerson, text], k) => (
                          <p key={k} class="whitespace-pre-wrap">
                            <span class="text-[var(--text-primary)]">{byPerson ? "You" : x.aiName || "The AI"}:</span> {text}
                          </p>
                        ))}
                      </blockquote>
                      <p class="mt-1 text-xs text-[var(--text-muted)]">
                        From "{x.title || "a conversation"}"{x.aiName ? `, with ${x.aiName}` : ""}.
                      </p>
                    </>
                  ) : (
                    <p class="mt-1 text-xs">
                      {x.state === "deleted"
                        ? "This passage came from a conversation you deleted. It cannot be shown."
                        : "Not on this computer yet. This passage came from a conversation on another of your devices that has not arrived here."}
                    </p>
                  )}
                </li>
              ))}
              {p.lines.map((line, i) => (
                <li key={`line-${i}`} data-testid="given-line-row">
                  <blockquote class="pl-3 border-l-2 border-[var(--border-subtle)] whitespace-pre-wrap">{line}</blockquote>
                </li>
              ))}
            </ul>
            {total > FIRST && !all.value && (
              <LiquidMetalButton variant="secondary" class="px-3 py-1 text-xs" testId="given-show-all" onClick$={() => { all.value = true; }}>
                Show all {total}
              </LiquidMetalButton>
            )}
            {p.leftOut.length > 0 && (
              <div data-testid="given-left-out">
                <div class="text-xs text-[var(--text-muted)] mb-1">Left out</div>
                <ul class="space-y-2 text-xs">
                  {p.leftOut.map((x, i) => {
                    const back = putBack.value.includes(x.item) || !x.stillReplaced;
                    const said = x.lines[0]?.[1];
                    const item = x.item;
                    return (
                      <li key={`left-${x.seq}-${i}`} data-testid="given-left-out-row" data-item={x.item}>
                        <p>
                          {dayWords(x.day)}
                          {said ? `: "${said}"` : ""}{" "}
                          {back
                            ? "This is current again, so it will be given next time."
                            : `Replaced${x.replacedOn ? ` on ${dayWords(x.replacedOn)}` : ""}, so it was left out of what ${props.ai} was given.`}
                        </p>
                        {!back && (
                          <LiquidMetalButton
                            variant="secondary"
                            class="mt-1 px-3 py-1 text-xs"
                            testId="given-not-replaced"
                            onClick$={async () => {
                              try {
                                await invoke("memory_not_replaced", { id: item });
                                putBack.value = [...putBack.value, item];
                              } catch (e) {
                                failed.value = String(e);
                              }
                            }}
                          >
                            Not replaced
                          </LiquidMetalButton>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
            <div>
              <div class="text-xs text-[var(--text-muted)] mb-1">Also given</div>
              <ul class="space-y-1 text-xs">
                {!!also.rules && (
                  <li>
                    Your {also.rules} {also.rules === 1 ? "rule" : "rules"}. <a href="/your-memory/" class="underline">See them</a>
                  </li>
                )}
                <li>Today's date.</li>
                <li>
                  The instruction to quote what it uses and give its date.{" "}
                  {p.instruction && (
                    <button type="button" class="underline" data-testid="given-exact-text" onClick$={() => { exact.value = !exact.value; }}>
                      {exact.value ? "Hide the exact text" : "Show the exact text"}
                    </button>
                  )}
                  {exact.value && p.instruction && (
                    <blockquote class="mt-1 pl-3 border-l-2 border-[var(--border-subtle)] whitespace-pre-wrap text-[var(--text-secondary)]">{p.instruction}</blockquote>
                  )}
                </li>
              </ul>
            </div>
          </>
        )}
      </div>
    </div>
  );
});
