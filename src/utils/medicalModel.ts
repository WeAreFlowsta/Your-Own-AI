/**
 * The user's preferred offline model for health questions.
 *
 * Health turns always stay on the device; this setting says WHICH installed
 * model answers them. It starts as the first model the user downloaded, and
 * when a medical specialist (MedGemma) is installed the app asks once
 * whether to make it the default - a visible choice, never a silent hijack.
 *
 * Stored in localStorage for the UI and mirrored into the Tauri store
 * (settings.json) where the Rust router reads it per medical turn.
 */
import { modelManager } from "./modelManager";

const KEY = "medicalPreferredModel";
const PROMPTED_KEY = "medicalModelPromptDone";

export function isMedicalSpecialist(filename: string): boolean {
  return filename.toLowerCase().includes("medgemma");
}

export function getMedicalModel(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export async function setMedicalModel(filename: string): Promise<void> {
  try {
    localStorage.setItem(KEY, filename);
  } catch {
    /* UI-side convenience */
  }
  // Mirror into the store the Rust router reads (same pattern as the
  // attachments-online consent).
  try {
    const { Store } = await import("@tauri-apps/plugin-store");
    const store = await Store.load("settings.json");
    await store.set("medicalPreferredModel", filename);
    await store.save();
  } catch (err) {
    console.warn("[medical-model] store mirror failed:", err);
  }
}

/** One-time ask after a MedGemma download; true = we already asked. */
export function medicalPromptDone(): boolean {
  try {
    return localStorage.getItem(PROMPTED_KEY) === "true";
  } catch {
    return true;
  }
}

export function setMedicalPromptDone(): void {
  try {
    localStorage.setItem(PROMPTED_KEY, "true");
  } catch {
    /* ignore */
  }
}

/**
 * The setting's effective value, initializing it on first sight:
 * a specialist if one is already installed (that IS today's routing -
 * no silent change for existing MedGemma users), else the first
 * installed chat model. Persists the initialization so routing is
 * explicit from then on. Returns null only when no models exist yet.
 */
export async function ensureMedicalModel(): Promise<string | null> {
  const existing = getMedicalModel();
  if (existing) {
    // A deleted model must not keep answering health questions from the
    // settings file - fall back to initialization.
    if (await modelManager.isModelDownloaded(existing)) return existing;
  }
  const installed = await modelManager.listModels();
  if (installed.length === 0) return null;
  const specialist = installed.find((m) => isMedicalSpecialist(m.name));
  // "First model downloaded": oldest file on disk (modified_at ascending).
  const oldest = [...installed].sort((a, b) =>
    String(a.modified_at ?? "").localeCompare(String(b.modified_at ?? "")),
  )[0];
  const pick = (specialist ?? oldest).name;
  await setMedicalModel(pick);
  return pick;
}

/**
 * Pin the user's CURRENT (non-specialist) model as the health answerer -
 * the "keep my current model" branch of the one-time specialist offer.
 * Without an explicit pin the specialist would win by ranking anyway,
 * which is precisely what the user just declined.
 */
export async function pinCurrentNonSpecialist(): Promise<void> {
  const existing = getMedicalModel();
  if (existing && !isMedicalSpecialist(existing)) return;
  const installed = await modelManager.listModels();
  const nonSpecialists = installed.filter((m) => !isMedicalSpecialist(m.name));
  if (nonSpecialists.length === 0) return;
  const oldest = [...nonSpecialists].sort((a, b) =>
    String(a.modified_at ?? "").localeCompare(String(b.modified_at ?? "")),
  )[0];
  await setMedicalModel(oldest.name);
}
