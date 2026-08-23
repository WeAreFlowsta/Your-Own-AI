/**
 * Session model cache (stale-while-revalidate)
 *
 * The model picker (Edit AI modal) used to re-fetch the local model list, the
 * per-model fit grades, and the online catalog from scratch every time it opened —
 * each one re-reading GGUF headers — so the dropdown sat on "No models available"
 * for a beat on a cold open. This is a tiny module-level cache: opens read whatever
 * is cached *instantly*, then a background refresh keeps it current. Populated once
 * at app startup via `prefetchModels()`, refreshed on demand.
 *
 * Module-level state persists for the session (single-window desktop app), so it
 * survives modal mount/unmount. It is NOT persisted across app restarts.
 */
import { invoke } from "@tauri-apps/api/core";
import type { LocalModel } from "../types";

/** green = fits fully on the GPU; split = GPU + RAM (MoE experts in main
 *  memory, fast for its size); yellow = runs slower (CPU-only, tight RAM);
 *  red = will not run here. */
export type FitGrade = "green" | "split" | "yellow" | "red";
export type FitMap = Record<string, FitGrade>;
export interface OnlineModel {
  id: string;
  display_name: string;
  description: string;
  /** Shelf/category from the catalog ("chat" | "web_search" | "coding") —
   *  web_search drives the "Searching the web" status during streams. */
  category?: string;
}

let localCache: LocalModel[] | null = null;
let fitsCache: FitMap | null = null;
let onlineCache: OnlineModel[] | null = null;

/** Whatever's cached right now (null = never fetched yet). */
export function getCachedModels(): {
  local: LocalModel[] | null;
  fits: FitMap | null;
  online: OnlineModel[] | null;
} {
  return { local: localCache, fits: fitsCache, online: onlineCache };
}

export async function refreshLocalModels(): Promise<LocalModel[]> {
  // Damaged files (incomplete or corrupted downloads) are never a choice.
  const models = (await invoke<LocalModel[]>("list_local_models")).filter((m) => !m.damaged);
  localCache = models;
  return models;
}

export async function refreshFits(): Promise<FitMap> {
  const fits = await invoke<{ name: string; fit: FitGrade }[]>("assess_model_fit");
  fitsCache = Object.fromEntries(fits.map((f) => [f.name, f.fit]));
  return fitsCache;
}

export async function refreshOnlineModels(): Promise<OnlineModel[]> {
  const models = await invoke<OnlineModel[]>("list_online_models");
  onlineCache = models;
  return models;
}

/**
 * Warm all three caches in the background. Safe to call repeatedly; failures are
 * swallowed (offline / proxy down just leaves that slice null). Call at app start
 * so the first model-picker open is instant.
 */
export function prefetchModels(): void {
  void refreshLocalModels().catch(() => {});
  void refreshFits().catch(() => {});
  // The online catalog warms ONLY for a signed-in session. Signed out,
  // online models cannot be used, so the warm-up was a pointless launch
  // request to the relay - identity-free, but still "an install woke
  // up". The catalog now loads on demand (Online Models page, pickers,
  // routing) or once signed in. Online is a choice, and the network
  // traffic should say the same.
  void invoke<{ signed_in: boolean }>("flowsta_session")
    .then((s) => (s.signed_in ? refreshOnlineModels() : undefined))
    .catch(() => {});
}
