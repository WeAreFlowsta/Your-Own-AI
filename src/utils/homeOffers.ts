/**
 * The home page has ONE offer slot under the hero (InitialView). What may
 * take it, in priority order:
 *
 *   1. The NVIDIA engine - install or update. Always wins while eligible,
 *      and it stays until installed or dismissed.
 *   2. The helper model - only when nothing above wants the slot, never on
 *      the very first launch (the first run is the wizard's; nothing else
 *      competes for attention), and never once installed or dismissed.
 *
 * Notices that are not offers (GPU fallback warning, app update available)
 * are separate surfaces and do not take part.
 */
import { invoke } from '@tauri-apps/api/core';
import { modelManager } from './modelManager';
import { UTILITY_MODEL } from '../data/recommended-models';

const LAUNCH_COUNT_KEY = 'yoaiLaunchCount';

/** How many times the app has started on this profile (1 = first run). */
export function launchCount(): number {
  try {
    return Number(localStorage.getItem(LAUNCH_COUNT_KEY) || '0');
  } catch {
    return 0;
  }
}

/** Called once per app start (root layout mount). */
export function bumpLaunchCount(): number {
  const n = launchCount() + 1;
  try { localStorage.setItem(LAUNCH_COUNT_KEY, String(n)); } catch { /* storage off */ }
  return n;
}

export interface CudaOffer {
  eligible: boolean;
  /** An older engine version is installed: say "update", never "install". */
  isUpdate: boolean;
  tag: string;
}

/** Is the NVIDIA engine offer (install or update) due on this machine? */
export async function cudaOffer(): Promise<CudaOffer> {
  try {
    const [status, info, safe] = await Promise.all([
      invoke<{ supported: boolean; gpu_supported: boolean; installed: boolean; stale_version_installed: boolean; tag: string }>('engine_status'),
      invoke<{ gpu_name?: string | null }>('get_system_info'),
      invoke<{ cuda_disabled?: boolean }>('gpu_safe_mode_status').catch(() => ({ cuda_disabled: false })),
    ]);
    const nvidia = (info.gpu_name ?? '').toLowerCase().includes('nvidia');
    return {
      // gpu_supported: never pitch an engine the card's generation cannot
      // execute (a GTX 960M install crashed every load until this gate).
      eligible: nvidia && status.supported && status.gpu_supported && !status.installed && !safe.cuda_disabled,
      isUpdate: status.stale_version_installed && !status.installed,
      tag: status.tag,
    };
  } catch {
    return { eligible: false, isUpdate: false, tag: '' };
  }
}

/** Is the helper-model offer due? Second launch onward, nothing else in
 *  the slot, not installed. Dismissal is the Callout's own (help tip id). */
export async function helperOfferEligible(): Promise<boolean> {
  if (launchCount() < 2) return false;
  try {
    if (await modelManager.isModelDownloaded(UTILITY_MODEL.filename)) return false;
  } catch {
    return false;
  }
  const cuda = await cudaOffer();
  return !cuda.eligible;
}
