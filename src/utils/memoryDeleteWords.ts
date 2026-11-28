/** What deleting an AI does to the memory items it kept, in words; empty when it kept none. */
export function aiKeptWords(name: string, kept: { stay: number; go: number } | null): string {
  if (!kept) return '';
  const all = kept.stay + kept.go;
  if (all === 0) return '';
  const items = (n: number) => `${n} memory ${n === 1 ? 'item' : 'items'}`;
  const stays = (n: number) => `will be kept, without the ${n === 1 ? 'conversation it' : 'conversations they'} came from`;
  if (kept.go === 0) return `${name} kept ${items(all)} for all your AIs. ${all === 1 ? 'It' : 'They'} ${stays(all)}. `;
  if (kept.stay === 0) return `${name} kept ${items(all)}, for ${name} only. ${all === 1 ? 'It' : 'They'} will be deleted. `;
  return `${name} kept ${items(all)}. ${kept.stay} ${kept.stay === 1 ? 'is' : 'are'} for all your AIs and ${stays(kept.stay)}. ${kept.go} ${kept.go === 1 ? 'was' : 'were'} for ${name} only and will be deleted. `;
}
