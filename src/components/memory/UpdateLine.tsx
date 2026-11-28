/**
 * The one line shown after the update, once: memory has been rebuilt.
 * It can be dismissed or left alone; nothing waits for it. It goes by
 * itself once "See what changed" is opened, or after a week on screen.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { Callout } from "../Callout";
import { UPDATE_EVENT, dismissUpdateLine, hadOlderMemory, updateLineDismissed, updateLineShown, updateReport } from "../../utils/memoryUpdate";

export const UpdateLine = component$<{ class?: string }>((props) => {
  const nav = useNavigate();
  const show = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const check = async () => {
      show.value = !updateLineDismissed() && hadOlderMemory(await updateReport());
      if (show.value) updateLineShown();
    };
    void check();
    window.addEventListener(UPDATE_EVENT, check);
    cleanup(() => window.removeEventListener(UPDATE_EVENT, check));
  });

  const dismiss = $(() => dismissUpdateLine());

  if (!show.value) return null;
  return (
    <div data-testid="memory-update-line" class={props.class ?? ""}>
      <Callout intent="info">
        <p>Memory has been rebuilt. Your AIs can now look through all your conversations.</p>
        <div class="mt-2 flex items-center gap-4">
          <button type="button" data-testid="memory-update-see" onClick$={async () => {
            // Opened is read: the line does not come back.
            dismissUpdateLine();
            await nav("/memory-update/");
          }} class="text-xs font-medium text-[var(--text-link)] hover:underline">
            See what changed
          </button>
          <button type="button" data-testid="memory-update-dismiss" onClick$={dismiss} class="text-xs font-medium text-[var(--text-muted)] hover:text-[var(--text-secondary)] hover:underline">
            Dismiss
          </button>
        </div>
      </Callout>
    </div>
  );
});
