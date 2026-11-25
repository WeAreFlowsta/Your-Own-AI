/** What deleting an AI does to the memory items it kept, in words; empty when it kept none. */
export function aiKeptWords(name: string, kept: { stay: number; go: number } | null): string {
  if (!kept) return '';
  const all = kept.stay + kept.go;
  if (all === 0) return '';
  return `${name} kept ${all} memory ${all === 1 ? 'item' : 'items'}. ${kept.stay} ${kept.stay === 1 ? 'is' : 'are'} for all your AIs and will be kept, without the conversations they came from. ${kept.go} ${kept.go === 1 ? 'was' : 'were'} for ${name} only and will be deleted. `;
}
