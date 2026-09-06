/**
 * The person's document library (src-tauri/src/corpus.rs): owned at the user
 * level, each AI granted access per document. "Add documents" on an AI's
 * Knowledge tab means "add to my library and give this AI access".
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface DocMeta {
  filename: string;
  path?: string;
  author?: string;
  title?: string;
  summary?: string;
  mine: boolean;
}

export interface DocRecord {
  doc_id: string;
  added_at: number;
  byte_size: number;
  chunk_count: number;
  meta: DocMeta;
  ai_ids: string[];
}

export interface ImportReport {
  added: DocRecord[];
  failed: { file: string; reason: string }[];
  already: number;
  cancelled: boolean;
}

export interface CorpusProgress {
  phase: 'reading' | 'embedding' | 'done';
  file: string;
  done: number;
  total: number;
  added: number;
  failed: number;
}

export interface RecallHit {
  doc_id: string;
  filename: string;
  idx: number;
  text: string;
  score: number;
  mine: boolean;
}

/** Files and folders in; progress on `corpus-progress`; cancel with corpusCancel(). */
export function corpusImport(paths: string[], aiId: string, names: string[] = []): Promise<ImportReport> {
  return invoke<ImportReport>('corpus_import', { paths, aiId, names });
}

export interface RereadReport {
  restored: number;
  unmatched: number;
  remaining: number;
  failed: { file: string; reason: string }[];
  cancelled: boolean;
}

/** Read restored records' files again from the folders given (LibraryRereadNotice). */
export function corpusReread(paths: string[]): Promise<RereadReport> {
  return invoke<RereadReport>('corpus_reread', { paths });
}

export function corpusCancel(): Promise<void> {
  return invoke('corpus_cancel');
}

export function onCorpusProgress(cb: (p: CorpusProgress) => void): Promise<() => void> {
  return listen<CorpusProgress>('corpus-progress', (e) => cb(e.payload));
}

export function corpusDocuments(aiId?: string): Promise<DocRecord[]> {
  return invoke<DocRecord[]>('corpus_documents', { aiId: aiId ?? null });
}

/** Withholding the last grant removes the document from the library. */
export function corpusGrant(docId: string, aiId: string, on: boolean): Promise<void> {
  return invoke('corpus_grant', { docId, aiId, on });
}

export function corpusDelete(docId: string): Promise<void> {
  return invoke('corpus_delete', { docId });
}

/** A card written on the device (src/utils/documentSummaries.ts). */
export function corpusSetSummary(docId: string, summary: string | null): Promise<DocMeta> {
  return invoke<DocMeta>('corpus_set_summary', { docId, summary });
}

export function corpusSetMine(docId: string, mine: boolean): Promise<DocMeta> {
  return invoke<DocMeta>('corpus_set_mine', { docId, mine });
}

export function corpusRecall(aiId: string, query: number[], maxPassages = 8): Promise<RecallHit[]> {
  return invoke<RecallHit[]>('corpus_recall', { aiId, query, maxPassages });
}

export function corpusDocumentText(docId: string, maxChars: number): Promise<{ doc_id: string; filename: string; text: string; truncated: boolean; chunk_count: number }> {
  return invoke('corpus_document_text', { docId, maxChars });
}

const MIGRATED_KEY = 'corpusMigratedDocs';

/**
 * One-time move of document knowledge out of the per-AI embedding blobs into
 * the library (text and vectors are reused, nothing is re-embedded). Runs
 * once per launch until every AI's document chunks are gone; entries without
 * vectors (a restore in progress) wait for the re-embed and the next pass.
 */
export async function migrateLegacyDocuments(aiIds: string[]): Promise<number> {
  try {
    if (localStorage.getItem(MIGRATED_KEY) === '1') return 0;
  } catch {
    /* no storage */
  }
  const { getTranscriptEmbeddings, saveTranscriptEmbeddings } = await import('./transcriptMemory');
  let moved = 0;
  let leftover = false;
  for (const aiId of aiIds) {
    const all = await getTranscriptEmbeddings(aiId);
    const docs = new Map<string, { filename: string; size: number; passages: string[]; vectors: number[][]; ok: boolean }>();
    for (const e of all) {
      if (e.kind !== 'authored' || !e.source) continue;
      const d = docs.get(e.source.doc_id) ?? { filename: e.source.filename, size: e.source.size_bytes, passages: [], vectors: [], ok: true };
      d.passages.push(e.text);
      d.vectors.push(e.vector);
      if (!e.vector || e.vector.length === 0) d.ok = false;
      docs.set(e.source.doc_id, d);
    }
    if (docs.size === 0) continue;
    const movedIds = new Set<string>();
    for (const [docId, d] of docs) {
      if (!d.ok) {
        leftover = true;
        continue;
      }
      try {
        await invoke('corpus_import_prepared', { aiId, filename: d.filename, byteSize: d.size, passages: d.passages, vectors: d.vectors });
        movedIds.add(docId);
        moved += 1;
      } catch (e) {
        console.warn('[corpus] migration failed for', d.filename, e);
        leftover = true;
      }
    }
    if (movedIds.size > 0) {
      await saveTranscriptEmbeddings(aiId, all.filter((e) => !(e.kind === 'authored' && e.source && movedIds.has(e.source.doc_id))));
    }
  }
  if (!leftover) {
    try {
      localStorage.setItem(MIGRATED_KEY, '1');
    } catch {
      /* no storage */
    }
  }
  if (moved) console.log(`[corpus] moved ${moved} document(s) into the library`);
  return moved;
}
