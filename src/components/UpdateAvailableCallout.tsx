/**
 * The quiet update nudge on the hero: same Callout treatment as the GPU
 * and CUDA notices beside it. Dismissible PER VERSION (the Callout id
 * carries the version), so waving away 0.5.1 does not swallow 0.5.2.
 * Never a modal, never a repeat nag, silent when offline or turned off.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { Callout } from "./Callout";
import { availableUpdate } from "../utils/updateCheck";

export const UpdateAvailableCallout = component$(() => {
  const version = useSignal<string | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    version.value = await availableUpdate();
  });

  if (!version.value) return null;
  return (
    <Callout
      intent="info"
      title={`Your Own AI ${version.value} is available`}
      id={`update-available-${version.value}`}
      class="mt-4"
    >
      A newer version is ready.{" "}
      <button
        type="button"
        class="underline"
        onClick$={async () => {
          const { openUrl } = await import("@tauri-apps/plugin-opener");
          await openUrl("https://yourownai.net/download/");
        }}
      >
        See what's new and download it
      </button>{" "}
      whenever suits - nothing updates by itself.
    </Callout>
  );
});

export default UpdateAvailableCallout;
