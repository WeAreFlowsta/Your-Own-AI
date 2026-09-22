
/** File types the document extractor handles (mirrors read_file_for_context). */
const DOC_EXTENSIONS = [
  'txt','md','csv','json','xml','yaml','yml','toml','log','ini','cfg','conf',
  'pdf','docx','doc','xlsx','xls','ods','odt','rtf','html','htm','sql','epub',
  'py','js','ts','tsx','jsx','rs','go','java','c','cpp','h','cs','rb','php',
];

/**
 * Shared "Add documents" flow: open the file picker, extract each file's text
 * (Rust side), and ingest it as this AI's knowledge. Used by both the edit-AI
 * dialog's Knowledge tab and the memory page's Documents section.
 *
 * Returns null if the user cancelled the picker; otherwise the filenames that
 * failed (empty = all good). Failures are usually the embedding model still
 * downloading, or an unreadable/scanned file.
 */
export async function pickAndIngestDocuments(aiId: string): Promise<IngestOutcome | null> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const selected = await open({
    multiple: true,
    filters: [{ name: 'Documents', extensions: DOC_EXTENSIONS }],
  });
  if (!selected) return null;
  return ingestDocumentPaths(aiId, Array.isArray(selected) ? selected : [selected]);
}

/** Is this a file the document reader handles? (dropped files skip the picker's filter) */
export function isDocumentPath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return DOC_EXTENSIONS.includes(ext);
}

/**
 * Add files and folders (walked) to the library and grant them to this AI.
 * The reading, cutting and embedding happen in Rust (src-tauri/src/corpus.rs)
 * one document at a time; progress arrives on `corpus-progress`.
 */
export type IngestOutcome = { failures: string[]; added: number; already: number; reread: number; relinked?: number; cancelled: boolean; folders?: number };

export async function ingestDocumentPaths(aiId: string, paths: string[]): Promise<IngestOutcome> {
  const { corpusImport, corpusFolderAdd, corpusFolderSync } = await import('./corpus');
  const { invoke } = await import('@tauri-apps/api/core');
  const { userNames } = await import('./userNames');
  // A file is a copy taken now. A FOLDER is a living thing - a notes vault,
  // a project's papers - so it is kept in sync: no second gesture to learn.
  const folders: string[] = [];
  const files: string[] = [];
  for (const p of paths) {
    const isDir = await invoke<boolean>('path_is_dir', { path: p }).catch(() => false);
    (isDir ? folders : files).push(p);
  }
  const out: IngestOutcome = { failures: [], added: 0, already: 0, reread: 0, relinked: 0, cancelled: false, folders: folders.length };
  if (files.length) {
    const report = await corpusImport(files, aiId, await userNames());
    out.failures.push(...report.failed.map((f) => `${f.file} (${f.reason})`));
    out.added += report.added.length;
    out.already += report.already;
    out.reread += report.reread ?? 0;
    out.cancelled = report.cancelled;
  }
  for (const folder of folders) {
    if (out.cancelled) break;
    const id = await corpusFolderAdd(folder, aiId);
    for (const r of await corpusFolderSync(id)) {
      if (r.unreachable) out.failures.push(`${folder} (could not be read)`);
      out.failures.push(...r.failed.map((f) => `${f.file} (${f.reason})`));
      out.added += r.added;
      out.already += r.unchanged;
      out.reread += r.updated;
      out.relinked = (out.relinked ?? 0) + (r.relinked ?? 0);
      out.cancelled = out.cancelled || r.cancelled;
    }
  }
  return out;
}

/** What a failure means, said plainly. The memory component is offered in
 *  place (MemoryComponentOffer), so this covers the files themselves. */
export function ingestFailureMessage(failures: string[]): string {
  return `Couldn't read ${failures.join(', ')}.`;
}

/** What a drop or a pick did, in one plain line (failures are said separately). */
export function ingestOutcomeMessage(o: IngestOutcome): string {
  const n = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;
  const parts = [
    o.added ? `${n(o.added, 'document', 'documents')} added` : '',
    o.reread ? `${n(o.reread, 'document', 'documents')} read again` : '',
    o.relinked ? `${n(o.relinked, 'document', 'documents')} found again` : '',
  ].filter(Boolean);
  const kept = o.folders ? (o.folders === 1 ? ' This folder is kept in sync.' : ' These folders are kept in sync.') : '';
  if (parts.length) return `${parts.join(', ')}.${kept}${o.cancelled ? ' Stopped before the end.' : ''}`;
  if (o.cancelled) return 'Stopped - nothing was added.';
  if (o.already) return `Already up to date - ${n(o.already, 'document', 'documents')}, nothing new.${kept}`;
  return o.failures.length ? '' : `Nothing to read there.${kept}`;
}
