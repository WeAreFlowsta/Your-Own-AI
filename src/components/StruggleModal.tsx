/**
 * The turn found the small model wanting, and the person is told so where
 * they cannot miss it (a line inside a folded rail went unseen, 09-25):
 * one plain paragraph and the way up as buttons. They can always click
 * away. Never on our own faults; once per session.
 */
import { component$, type QRL } from "@builder.io/qwik";
import { LuInfo } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";

export interface StruggleInfo {
  /** The paragraph, in the person's words. */
  text: string;
  /** The stronger model on this computer, when the AI is pinned to a weaker one. */
  bigger?: string;
  /** The account has online models. */
  entitled: boolean;
}

export type StruggleAction = "see-online" | "go-online" | "switch-local" | "close";

const pretty = (f: string) => f.replace(/\.gguf$/i, "").replace(/-Q\d[^-]*$/i, "");

export default component$<{ struggle: StruggleInfo | null; onAction$: QRL<(action: StruggleAction) => void> }>(
  ({ struggle, onAction$ }) => {
    if (!struggle) return null;
    return (
      <div class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50" onClick$={() => onAction$("close")}>
        <div
          class="bg-[var(--bg-header-footer)] p-6 md:p-7 rounded-xl shadow-2xl w-full max-w-md relative"
          onClick$={(e: MouseEvent) => e.stopPropagation()}
        >
          <div class="flex items-start gap-4 mb-6">
            <div class="w-11 h-11 rounded-full flex items-center justify-center flex-shrink-0 bg-[var(--bg-dropdown)] text-[var(--text-secondary)]">
              <LuInfo class="w-5 h-5" />
            </div>
            <div class="min-w-0 flex-1 pt-0.5">
              <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela">Your AI found this hard</h2>
              <p class="text-sm text-[var(--text-secondary)] mt-1">{struggle.text}</p>
            </div>
          </div>
          {/* Secondary first in DOM; the primary sits on the right. */}
          <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2">
            <LiquidMetalButton variant="secondary" onClick$={() => onAction$("close")} class="px-4 py-2 text-sm">
              Not now
            </LiquidMetalButton>
            {struggle.bigger && (
              <LiquidMetalButton
                variant={struggle.entitled ? "secondary" : "primary"}
                onClick$={() => onAction$("switch-local")}
                class="px-4 py-2 text-sm"
              >
                Switch to {pretty(struggle.bigger)}
              </LiquidMetalButton>
            )}
            {struggle.entitled ? (
              <LiquidMetalButton variant="primary" onClick$={() => onAction$("go-online")} class="px-4 py-2 text-sm">
                Use online models for this AI
              </LiquidMetalButton>
            ) : (
              <LiquidMetalButton variant="primary" onClick$={() => onAction$("see-online")} class="px-4 py-2 text-sm">
                See online models
              </LiquidMetalButton>
            )}
          </div>
        </div>
      </div>
    );
  },
);
