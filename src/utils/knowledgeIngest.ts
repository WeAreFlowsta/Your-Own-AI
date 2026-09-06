import { addDocumentKnowledge } from './transcriptMemory';

/** File types the document extractor handles (mirrors read_file_for_context). */
const DOC_EXTENSIONS = [
  'txt','md','csv','json','xml','yaml','yml','toml','log','ini','cfg','conf',
  'pdf','docx','doc','xlsx','xls','ods','odt','rtf','html','htm','sql',
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
export async function pickAndIngestDocuments(aiId: string): Promise<{ failures: string[] } | null> {
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
 * Ingest files by path - from the picker or from a drop on the window.
 * Files the reader does not handle are reported, not attempted.
 */
export async function ingestDocumentPaths(aiId: string, paths: string[]): Promise<{ failures: string[] }> {
  const { invoke } = await import('@tauri-apps/api/core');
  const failures: string[] = [];
  for (const filePath of paths) {
    const name = filePath.split(/[/\\]/).pop() || filePath;
    if (!isDocumentPath(filePath)) {
      failures.push(name);
      continue;
    }
    try {
      const doc = await invoke<{ filename: string; size_bytes: number; content: string }>(
        'read_file_for_context',
        { filePath },
      );
      const result = await addDocumentKnowledge(aiId, doc.filename, doc.size_bytes, doc.content);
      if (!result) failures.push(doc.filename);
    } catch (e) {
      console.warn('[Knowledge] failed to ingest', filePath, e);
      failures.push(name);
    }
  }
  return { failures };
}

/** What a failure means, said plainly. The memory component is offered in
 *  place (MemoryComponentOffer), so this covers the file itself. */
export function ingestFailureMessage(failures: string[]): string {
  return `Couldn't read ${failures.join(', ')} - a scanned PDF, an empty file, or a type the reader doesn't know.`;
}
