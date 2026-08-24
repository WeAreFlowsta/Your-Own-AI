/**
 * Shown when the GPU safety ladder stepped down — either to CPU (safe mode
 * proper: GPU runs were hard-crashing the system) or, with the optional CUDA
 * engine installed, from CUDA to the bundled GPU engine. Lets the user step
 * back up ("Try again") once they've fixed their driver. There is no
 * permanent engine toggle by design; this notice and the Settings → Engines
 * card are the only places the choice surfaces.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { LuInfo, LuX } from "@qwikest/icons/lucide";

export default component$(() => {
  const active = useSignal(false);
  // CUDA rung just stepped down (GPU still in use via the bundled engine).
  // Shown only on the launch it engaged — the Engines card covers it after.
  const cudaStepped = useSignal(false);
  const dismissed = useSignal(false);
  const retried = useSignal(false);
  const busy = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const s = await invoke<{
        active: boolean;
        just_engaged: boolean;
        cuda_disabled?: boolean;
      }>("gpu_safe_mode_status");
      active.value = s.active;
      // Show whenever CUDA sits laddered out - not only the launch it
      // engaged - so there is always a way back (field 08-24: a bad model
      // file laddered CUDA out; the fix landed but the notice was gone).
      cudaStepped.value = !s.active && !!s.cuda_disabled;
    } catch {
      /* command unavailable (e.g. not on desktop) — stay hidden */
    }
  });

  const retry = $(async () => {
    busy.value = true;
    try {
      await invoke("gpu_retry");
      retried.value = true;
    } catch {
      /* ignore */
    }
    busy.value = false;
  });

  if ((!active.value && !cudaStepped.value) || dismissed.value) return null;

  return (
    <div class="mx-auto max-w-3xl mt-3 px-4 py-3 rounded-xl bg-amber-500/10 border border-amber-500/30 text-sm text-amber-300 flex items-start gap-3">
      <LuInfo class="w-4 h-4 mt-0.5 flex-shrink-0" />
      <div class="min-w-0 flex-1">
        {retried.value ? (
          <p class="font-medium">
            Re-enabled — restart the app to use it again.
          </p>
        ) : active.value ? (
          <>
            <p class="font-medium">Running on CPU for stability</p>
            <p class="text-amber-300/80 text-xs mt-0.5">
              The app detected a possible graphics-driver issue and switched to
              CPU to keep your system stable. AI responses may be slower.
            </p>
          </>
        ) : (
          <>
            <p class="font-medium">Back on the standard engine</p>
            <p class="text-amber-300/80 text-xs mt-0.5">
              The CUDA engine crashed repeatedly, so your chats now use the
              standard engine. Your graphics card is still in use.
            </p>
          </>
        )}
      </div>
      {!retried.value && (
        <button
          onClick$={retry}
          disabled={busy.value}
          class="flex-shrink-0 px-3 py-1.5 rounded-lg text-xs border border-amber-500/40 hover:bg-amber-500/15 transition-colors disabled:opacity-50"
        >
          {active.value ? "Try GPU again" : "Try CUDA again"}
        </button>
      )}
      <button
        onClick$={() => (dismissed.value = true)}
        class="flex-shrink-0 text-amber-300/60 hover:text-amber-300"
        aria-label="Dismiss"
      >
        <LuX class="w-4 h-4" />
      </button>
    </div>
  );
});
