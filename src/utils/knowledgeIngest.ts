
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
export async function pickAndIngestDocuments(aiId: string): Promise<{ failures: string[]; added: number; already: number; cancelled: boolean } | null> {
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
export async function ingestDocumentPaths(aiId: string, paths: string[]): Promise<{ failures: string[]; added: number; already: number; cancelled: boolean }> {
  const { corpusImport } = await import('./corpus');
  const { userNames } = await import('./userNames');
  const report = await corpusImport(paths, aiId, await userNames());
  return {
    failures: report.failed.map((f) => `${f.file} (${f.reason})`),
    added: report.added.length,
    already: report.already,
    cancelled: report.cancelled,
  };
}

/** What a failure means, said plainly. The memory component is offered in
 *  place (MemoryComponentOffer), so this covers the files themselves. */
export function ingestFailureMessage(failures: string[]): string {
  return `Couldn't read ${failures.join(', ')}.`;
}
