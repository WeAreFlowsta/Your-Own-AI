/**
 * The optional app-update check: at most once a day, ask yourownai.net for
 * the latest published version (a static file; the request carries nothing
 * about this install) and surface a quiet Callout when it is newer.
 * Settings > Help & diagnostics turns it off ("checkAppUpdates" != "false").
 */
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

const LAST_CHECK_KEY = "updateCheckLastAt";
const LATEST_KEY = "updateCheckLatest";
const CHECK_EVERY_MS = 24 * 60 * 60 * 1000;

export function updateChecksEnabled(): boolean {
  try {
    return localStorage.getItem("checkAppUpdates") !== "false";
  } catch {
    return false;
  }
}

export function setUpdateChecksEnabled(on: boolean): void {
  try {
    if (on) localStorage.removeItem("checkAppUpdates");
    else localStorage.setItem("checkAppUpdates", "false");
  } catch {
    /* ignore */
  }
}

function newer(latest: string, current: string): boolean {
  const p = (v: string) => v.replace(/^v/, "").split(".").map((n) => parseInt(n, 10) || 0);
  const [a, b, c] = p(latest);
  const [x, y, z] = p(current);
  return a !== x ? a > x : b !== y ? b > y : c > z;
}

/** The newer version to offer, or null. Never throws; offline = null. */
export async function availableUpdate(): Promise<string | null> {
  if (!updateChecksEnabled()) return null;
  let current = "";
  try {
    current = await getVersion();
  } catch {
    return null;
  }
  try {
    const lastAt = Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0);
    let latest = localStorage.getItem(LATEST_KEY);
    if (Date.now() - lastAt > CHECK_EVERY_MS) {
      const fetched = await invoke<string | null>("check_app_update");
      localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
      if (fetched) {
        latest = fetched;
        localStorage.setItem(LATEST_KEY, fetched);
      }
    }
    return latest && newer(latest, current) ? latest : null;
  } catch {
    return null;
  }
}
