/**
 * The app's own updates (src-tauri/src/updater.rs): a signed release is
 * checked for on start and every few hours, downloaded in the background,
 * and installed at "Restart to update" (or on quit, on Windows and macOS).
 * The requests are plain reads of two public files and carry nothing about
 * this install.
 */
import { invoke } from "@tauri-apps/api/core";

export type ReadyUpdate = { version: string; notes_url: string; changes_line: boolean; asks_password: boolean };
export type UpdatedFrom = { version: string; to: string; notes_url: string };
/** A newer release whose download waits for the computer, and for what. */
export type WaitingUpdate = { version: string; why: string };
export type UpdateStatus = {
  auto: boolean;
  channel: string;
  version: string;
  ready: ReadyUpdate | null;
  waiting: WaitingUpdate | null;
  notice: string | null;
  /** A newer version that is out while automatic updates are off. */
  out_while_off: string | null;
  updated: UpdatedFrom | null;
  can_update: boolean;
};

export const updateStatus = () => invoke<UpdateStatus>("update_status");
/** Start a waiting download now; false when none is waiting. */
export const downloadUpdateNow = () => invoke<boolean>("download_update_now");
export const setAutoUpdates = (auto: boolean) => invoke<UpdateStatus>("set_update_prefs", { auto });

/** The old daily version check's "off" carries over to automatic updates,
 * once; the old setting is then gone. */
export async function carryOverOldUpdateChoice(): Promise<void> {
  try {
    const old = localStorage.getItem("checkAppUpdates");
    if (old === null) return;
    if (old === "false") await setAutoUpdates(false);
    localStorage.removeItem("checkAppUpdates");
    localStorage.removeItem("updateCheckLastAt");
    localStorage.removeItem("updateCheckLatest");
  } catch {
    /* ignore */
  }
}
