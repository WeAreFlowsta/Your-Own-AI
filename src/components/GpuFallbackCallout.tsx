import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Callout } from './Callout';

/**
 * Amber notice in the same slot and style as the CUDA offer: the engine
 * declared this GPU unusable and the app switched to the processor. Shows
 * when the gpu-fallback event fires (live, mid-session) and on later
 * launches while the verdict stands. Dismissal is stored directly (NOT the
 * help-tips system - a hardware alert must not vanish for people who
 * turned help tips off), and clears if the verdict clears, so a user who
 * presses "Try my graphics card again" and trips it again is re-told.
 */
const DISMISS_KEY = 'gpu-fallback-dismissed';

export default component$(() => {
  const reason = useSignal<'vulkan-driver' | 'cuda-arch' | null>(null);
  const dismissed = useSignal(true); // start hidden until state is known

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    try {
      const s = await invoke<{ device_unsupported?: string | null }>(
        'gpu_safe_mode_status',
      );
      if (s.device_unsupported === 'vulkan-driver' || s.device_unsupported === 'cuda-arch') {
        reason.value = s.device_unsupported;
        dismissed.value = localStorage.getItem(DISMISS_KEY) === s.device_unsupported;
      } else {
        // Verdict cleared (e.g. the user retried the GPU) - forget the
        // dismissal so a future verdict shows again.
        localStorage.removeItem(DISMISS_KEY);
      }
    } catch {
      /* stays hidden */
    }
    const un = await listen<{ reason?: string }>('gpu-fallback', (e) => {
      const r = e.payload?.reason;
      if (r === 'vulkan-driver' || r === 'cuda-arch') {
        reason.value = r;
        dismissed.value = false;
        localStorage.removeItem(DISMISS_KEY);
      }
    });
    cleanup(() => un());
  });

  const dismiss = $(() => {
    if (reason.value) localStorage.setItem(DISMISS_KEY, reason.value);
    dismissed.value = true;
  });

  if (!reason.value || dismissed.value) return null;

  return (
    <Callout
      intent="warning"
      title="Running on your processor"
      class="mt-10 text-left"
    >
      <p class="mb-2.5">
        {reason.value === 'vulkan-driver'
          ? "Your graphics card's driver can't run AI models, so they run on your processor instead - everything works, just slower. Installing the full driver from your graphics card maker (for NVIDIA cards, nvidia.com) may restore full speed - then choose \"Try my graphics card again\" in Settings, Engines."
          : "Your graphics card's generation isn't supported by the AI engine, so models run on your processor instead - everything works, just slower."}
      </p>
      <p class="mb-2.5">
        Smaller models run best on your processor - and{' '}
        {/* A door, not a pitch: this goes to the Online Models page (the
            storefront with its own explanation and gating), never toward a
            checkout from an error surface. */}
        <a
          href="/online-models"
          class="text-[var(--text-link)] hover:underline"
        >
          online models
        </a>{' '}
        run at full speed on any machine, if you ever want them.
      </p>
      <button
        type="button"
        onClick$={dismiss}
        class="text-sm text-[var(--text-link)] hover:underline"
      >
        Got it
      </button>
    </Callout>
  );
});
