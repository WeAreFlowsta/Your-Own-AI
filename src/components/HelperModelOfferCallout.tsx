/**
 * Home-page offer for the helper model (the optional on-device utility
 * model, Settings > Components). Second in the offer slot's priority - see
 * utils/homeOffers.ts for the rules. Plain words about what it does, one
 * download button, progress that survives a page change.
 */
import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { listen } from '@tauri-apps/api/event';
import { Callout } from './Callout';
import LiquidMetalButton from './LiquidMetalButton';
import { modelManager, type DownloadProgress } from '../utils/modelManager';
import { helperOfferEligible, helperFilesMissing } from '../utils/homeOffers';
import { UTILITY_MODEL, EMBEDDING_MODEL } from '../data/recommended-models';

const HELPER_FILES = [UTILITY_MODEL.filename, EMBEDDING_MODEL.filename];

export const HELPER_OFFER_TIP_ID = 'home-tip-helper-model';

export interface HelperModelOfferProps {
  /**
   * Where the person has just asked for something the helper does. Set, the
   * offer shows whenever the files are missing - no launch count, no shared
   * slot, and no "Got it": a dismissal on the home page was about the home
   * page, and here they are adding the documents it reads. Unset = the home
   * page offer (rules in utils/homeOffers.ts).
   */
  needFor?: 'documents' | 'placement';
}

export default component$<HelperModelOfferProps>(({ needFor }) => {
  const eligible = useSignal(false);
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const done = useSignal(false);
  const error = useSignal('');
  const totalGb = useSignal(0);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const missing = await helperFilesMissing();
    eligible.value = needFor ? missing.length > 0 : await helperOfferEligible();
    if (!eligible.value) return;
    totalGb.value = Math.round(missing.reduce((n, f) => n + f.size, 0) * 10) / 10;
    // A download started on an earlier visit is still running - reattach.
    for (const f of HELPER_FILES) {
      try {
        const st = await modelManager.downloadStatus(f);
        if (st.downloading) {
          downloading.value = true;
          percent.value = st.total_bytes > 0 ? Math.floor((st.downloaded_bytes / st.total_bytes) * 100) : 0;
        }
      } catch {
        /* nothing running */
      }
    }
    const unp = await listen<DownloadProgress>('model-download-progress', (e) => {
      if (HELPER_FILES.includes(e.payload.filename)) {
        downloading.value = true;
        percent.value = e.payload.percent;
      }
    });
    const unc = await listen<{ filename: string }>('model-download-complete', async (e) => {
      if (HELPER_FILES.includes(e.payload.filename) && (await helperFilesMissing()).length === 0) {
        downloading.value = false;
        done.value = true;
      }
    });
    cleanup(() => {
      unp();
      unc();
    });
  });

  const doDownload = $(async () => {
    error.value = '';
    downloading.value = true;
    percent.value = 0;
    try {
      // Both files, the small one first so memory and routing wake up
      // while the helper model is still coming down.
      const missing = (await helperFilesMissing()).sort((a, b) => a.size - b.size);
      for (const f of missing) {
        await modelManager.downloadModel(f.downloadUrl, f.filename, (p) => {
          percent.value = p.percent;
        });
      }
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
      intent="info"
      title={needFor === 'documents' ? 'Documents need a small download' : needFor === 'placement' ? 'The helper models are not installed yet' : 'Give your AIs a memory'}
      id={needFor ? undefined : HELPER_OFFER_TIP_ID}
      class={needFor === 'documents' ? 'mb-4 text-left' : needFor ? 'text-left' : 'mt-10 text-left'}
    >
      {done.value ? (
        <p>
          {needFor === 'documents'
            ? 'Installed. Add your documents now.'
            : needFor === 'placement'
              ? 'Installed. Where they run can be chosen here the next time you open Settings.'
              : 'Installed. Your AIs now remember what you tell them and can read documents you add. Nothing else to do.'}
        </p>
      ) : (
        <>
          <p class="mb-2.5">
            {needFor === 'placement'
              ? `A small optional download (${totalGb.value || UTILITY_MODEL.size} GB) so your AIs remember what you tell them and can read documents you add. It runs on this computer only.`
              : needFor
              ? `Your AIs read documents with a small helper (${totalGb.value || UTILITY_MODEL.size} GB). It runs on this computer only. Download it once, then add your documents.`
              : `A small optional download (${totalGb.value || UTILITY_MODEL.size} GB) so your AIs remember what you tell them and can read documents you add. It runs on this computer only. Remove it anytime in Settings.`}
          </p>
          {downloading.value ? (
            <div class="flex items-center gap-3">
              <div class="flex-1 h-1.5 rounded-full bg-[var(--bg-dropdown)] overflow-hidden">
                <div
                  class="h-full rounded-full bg-[var(--bg-button-primary)] transition-all"
                  style={{ width: `${Math.min(percent.value, 100)}%` }}
                />
              </div>
              <span class="text-xs text-[var(--text-muted)] shrink-0">Downloading... {Math.floor(percent.value)}%</span>
            </div>
          ) : (
            <LiquidMetalButton onClick$={doDownload} class="flex items-center px-3 py-1.5 text-xs">
              Download the helper
            </LiquidMetalButton>
          )}
          {error.value && <p class="text-xs text-red-600 dark:text-red-400 mt-2">{error.value}</p>}
        </>
      )}
    </Callout>
  );
});
