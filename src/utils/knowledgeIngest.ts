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
  const paths = Array.isArray(selected) ? selected : [selected];

  const { invoke } = await import('@tauri-apps/api/core');
  const failures: string[] = [];
  for (const filePath of paths) {
    try {
      const doc = await invoke<{ filename: string; size_bytes: number; content: string }>(
        'read_file_for_context',
        { filePath },
      );
      const result = await addDocumentKnowledge(aiId, doc.filename, doc.size_bytes, doc.content);
      if (!result) failures.push(doc.filename);
    } catch (e) {
      console.warn('[Knowledge] failed to ingest', filePath, e);
      failures.push(filePath.split(/[/\\]/).pop() || filePath);
    }
  }
  return { failures };
}

export function ingestFailureMessage(failures: string[]): string {
  return `Couldn't add ${failures.join(', ')}. If you just installed, the knowledge model may still be downloading (Settings - Components).`;
}
