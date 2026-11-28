/**
 * Memory opened on one project: which project the Memory page shows. The
 * choice is made where the project is (its menu) and read by the page, so
 * it is kept for the visit, not in the address.
 */
import { invoke } from "@tauri-apps/api/core";

export interface MemoryProject {
  /** The project's identity: the same on every device. */
  id: string;
  /** The folder's name, as the person knows the project. */
  name: string;
  folder: string;
}

const KEY = "memory-project";
export const MEMORY_PROJECT_EVENT = "yoai-memory-project";

export function chosenProject(): MemoryProject | null {
  try {
    const raw = sessionStorage.getItem(KEY);
    const p = raw ? (JSON.parse(raw) as MemoryProject) : null;
    return p && p.id ? p : null;
  } catch {
    return null;
  }
}

export function chooseProject(project: MemoryProject | null) {
  try {
    if (project) sessionStorage.setItem(KEY, JSON.stringify(project));
    else sessionStorage.removeItem(KEY);
  } catch {
    /* the choice lasts for this page only */
  }
  window.dispatchEvent(new CustomEvent(MEMORY_PROJECT_EVENT));
}

/** Choose the project a folder is, for the Memory page to open on. */
export async function openProjectMemory(folder: string): Promise<void> {
  const id = await invoke<string>("project_identity_for", { folder }).catch(() => "");
  if (!id) return;
  const name = folder.split(/[\\/]/).filter(Boolean).pop() ?? folder;
  chooseProject({ id, name, folder });
}
