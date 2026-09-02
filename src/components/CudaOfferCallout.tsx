import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { cudaOffer } from '../utils/homeOffers';
import { listen } from '@tauri-apps/api/event';
import { Callout } from './Callout';
import LiquidMetalButton from './LiquidMetalButton';

/** The zip filename the Rust download emits progress events under. */
const zipNameFor = (tag: string) =>
  `llama-server-cuda-${tag.replace(/^llama-/, '')}.zip`;

/**
 * Front-page discovery tip for the optional NVIDIA engine: shown only when
 * the GPU scan found an NVIDIA card, the engine is supported here, and it
 * isn't installed (or safety-disabled). Downloads in place so nobody has to
 * find Settings - Engines to get the speedup. Dismissible like every help
 * tip ("Got it"); Settings remains the full management surface.
 */
export default component$(() => {
  const eligible = useSignal(false);
  // True when an OLDER engine version is installed: the user already did
  // this once, so the copy must say "update", never "install" - a repeat of
  // the first-install pitch reads as "didn't I already do this?".
  const isUpdate = useSignal(false);
  const tag = useSignal('');
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const done = useSignal(false);
  const error = useSignal('');

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const offer = await cudaOffer();
    tag.value = offer.tag;
    isUpdate.value = offer.isUpdate;
    eligible.value = offer.eligible;
    // A download started on an earlier visit is still running in the app
    // (it never depended on this card) - reattach instead of offering again.
    if (tag.value) {
      try {
        const st = await invoke<{ downloading: boolean; downloaded_bytes: number; total_bytes: number }>(
          'download_status',
          { filename: zipNameFor(tag.value) },
        );
        if (st.downloading) {
          downloading.value = true;
          percent.value = st.total_bytes > 0 ? Math.floor((st.downloaded_bytes / st.total_bytes) * 100) : 0;
        }
      } catch {
        /* no status = nothing running */
      }
    }
    const unp = await listen<{ filename: string; percent: number }>(
      'model-download-progress',
      (e) => {
        if (tag.value && e.payload.filename === zipNameFor(tag.value)) {
          downloading.value = true;
          percent.value = e.payload.percent;
        }
      },
    );
    const unDone = await listen('engine-installed', () => {
      downloading.value = false;
      done.value = true;
    });
    const unFail = await listen<string>('engine-install-failed', (e) => {
      downloading.value = false;
      error.value = e.payload;
    });
    cleanup(() => {
      unp();
      unDone();
      unFail();
    });
  });

  const doDownload = $(async () => {
    error.value = '';
    downloading.value = true;
    percent.value = 0;
    try {
      await invoke('download_cuda_engine', {});
      done.value = true;
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      downloading.value = false;
    }
  });

  if (!eligible.value) return null;

  return (
    <Callout
      intent="premium"
      title={
        isUpdate.value
          ? 'Your NVIDIA engine needs an update'
          : 'Your NVIDIA graphics card can go faster'
      }
      // The update variant carries the engine version in its dismiss id, so
      // dismissing one update's tip (or the original install tip) never
      // hides the NEXT update's.
      id={isUpdate.value ? `home-tip-cuda-engine-${tag.value}` : 'home-tip-cuda-engine'}
      class="mt-10 text-left"
    >
      {done.value ? (
        <p>
          {isUpdate.value ? 'Updated' : 'Installed'}. The high-performance
          engine takes over the next time a model loads - nothing else to do.
        </p>
      ) : (
        <>
          <p class="mb-2.5">
            {isUpdate.value
              ? 'This app update comes with a newer version of the high-performance NVIDIA engine you installed earlier. One download (about 850 MB) and you keep your full speed - until then, models run on the standard engine.'
              : 'An optional high-performance engine is available for NVIDIA cards like yours - faster reading and quicker replies. One download (about 850 MB), no setup, and you can remove it anytime in Settings.'}
          </p>
          {downloading.value ? (
            <div class="flex items-center gap-3">
              <div class="flex-1 h-1.5 rounded-full bg-[var(--bg-dropdown)] overflow-hidden">
                <div
                  class={`h-full rounded-full bg-[var(--bg-button-primary)] transition-all ${percent.value >= 100 ? 'animate-pulse' : ''}`}
                  style={{ width: `${Math.min(percent.value, 100)}%` }}
                />
              </div>
              <span class="text-xs text-[var(--text-muted)] shrink-0">
                {percent.value < 100 ? `Downloading... ${Math.floor(percent.value)}%` : 'Installing...'}
              </span>
            </div>
          ) : (
            <LiquidMetalButton
              onClick$={doDownload}
              class="flex items-center px-3 py-1.5 text-xs"
            >
              {isUpdate.value ? 'Update the NVIDIA engine' : 'Download the NVIDIA engine'}
            </LiquidMetalButton>
          )}
          {error.value && (
            <p class="text-xs text-red-600 dark:text-red-400 mt-2">{error.value}</p>
          )}
        </>
      )}
    </Callout>
  );
});
