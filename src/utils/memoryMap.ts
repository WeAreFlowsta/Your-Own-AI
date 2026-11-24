/**
 * The Map's data: what memory has kept, by subject, for the chosen AI and
 * project. The backend decides what is counted; the page only draws it.
 */
import { invoke } from "@tauri-apps/api/core";

export interface MapSubject {
  id: string;
  /** This subject's id and those merged into it: an item belongs here when its subject is one of these. */
  ids: string[];
  name: string;
  nameAccepted: boolean;
  kind: "about_you" | "project" | "conversation" | "by_hand";
  project: string | null;
  items: number;
  open: number;
  audience: "all" | "ai" | "project" | "local" | "mixed";
  lastKept: number;
  lastCameUp: number | null;
}

export interface MapLink {
  a: string;
  b: string;
  times: number;
}

export interface MapProject {
  id: string;
  name: string;
  items: number;
  /** Which of the Map's project colors it has; null when all are taken. */
  color: number | null;
}

export interface MemoryMapData {
  subjects: MapSubject[];
  links: MapLink[];
  projects: MapProject[];
  unplaced: number;
}

export interface MapView {
  /** One AI, by its founding key. */
  ai?: string | null;
  /** One project, by its identity. */
  project?: string | null;
}

export function loadMap(view: MapView): Promise<MemoryMapData> {
  return invoke<MemoryMapData>("memory_map", { view: { ai: view.ai || null, project: view.project || null } });
}

export function renameSubject(id: string, name: string): Promise<void> {
  return invoke("memory_subject_rename", { id, name });
}

/** Who a subject's items are for, in the page's words. */
export function audienceWords(a: MapSubject["audience"]): string {
  switch (a) {
    case "ai":
      return "Only for one AI";
    case "project":
      return "For one project";
    case "local":
      return "For AIs on your own devices";
    case "mixed":
      return "Some for all your AIs, some not";
    default:
      return "For all your AIs";
  }
}

/** "today", "3 days ago", "5 months ago". */
export function ago(ms: number, now = Date.now()): string {
  const days = Math.floor((now - ms) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 14) return `${days} days ago`;
  if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
  if (days < 730) return `${Math.floor(days / 30)} months ago`;
  return `${Math.floor(days / 365)} years ago`;
}

export const countWords = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
