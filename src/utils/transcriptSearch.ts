/**
 * Full-text search across this AI's conversations (src-tauri/src/
 * transcript_search.rs): an encrypted text cache filled by reading the
 * records once, an in-memory index built from it on the first search.
 */
import { invoke } from '@tauri-apps/api/core';

export interface SearchHit {
  hash: string;
  seq: number;
  role: string;
  at: number;
  ai_id: string;
  ai_name: string;
  title: string | null;
  source: string | null;
  /** Matches wrapped in \u0001 … \u0002 - plain text, never HTML. */
  snippet: string;
  /** Other matching messages in the same conversation. */
  more: number;
}

export interface SearchAnswer {
  hits: SearchHit[];
  /** Nothing read for this AI yet: offer to read its conversations. */
  needs_read: boolean;
  building: boolean;
  /** The records are not answering yet (just after launch): the panel waits and asks again. */
  warming?: boolean;
}

export interface SearchProgress {
  done: number;
  total: number;
  finished: boolean;
  cancelled: boolean;
}

/** Search one AI's conversations (every agent generation of it). */
export function transcriptSearch(aiId: string, agentKey: string, query: string, limit?: number): Promise<SearchAnswer> {
  return invoke<SearchAnswer>('transcript_search', { aiId, agentKey, query, limit: limit ?? null });
}

/** Read this AI's conversations into the search cache (background; progress on `transcript-search-progress`). */
export function transcriptSearchBuild(agentKey: string): Promise<void> {
  return invoke<void>('transcript_search_build', { agentKey });
}

export function transcriptSearchCancel(): Promise<void> {
  return invoke<void>('transcript_search_cancel');
}

/** The snippet as parts: [text, isMatch][] - for rendering marks without HTML. */
export function snippetParts(snippet: string): [string, boolean][] {
  const out: [string, boolean][] = [];
  const re = /\u0001([^\u0002]*)\u0002/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(snippet))) {
    if (m.index > last) out.push([snippet.slice(last, m.index), false]);
    out.push([m[1], true]);
    last = m.index + m[0].length;
  }
  if (last < snippet.length) out.push([snippet.slice(last), false]);
  return out;
}
