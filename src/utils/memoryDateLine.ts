/**
 * A reply that is nothing but the line that stands above a passage memory
 * gave ("[2026-03-08, conversation "Pricing", with Atlas]"), or a piece of
 * it: the model began to cite and ended there. Such a reply answers
 * nothing, so the turn asks once more, as it does for an empty reply.
 *
 * True when the reply is one short line that starts with the date of one
 * of the lines above the passages in `given`, and every word in it is in
 * that same line. A date followed by anything else is an answer.
 */
const ABOVE_A_PASSAGE = /^\[(\d{4}-\d{2}-\d{2}), conversation "[^"\n]*", with [^\]\n]*\]$/gm;

export function onlyADateLine(reply: string, given: string | null | undefined): boolean {
  const said = reply.trim();
  if (!said || !given || said.length > 200 || said.includes("\n")) return false;
  const words = said.match(/[\p{L}\p{N}]+/gu) ?? [];
  if (words.length === 0) return false;
  const bare = said.replace(/^\[\s*/, "");
  for (const line of given.match(ABOVE_A_PASSAGE) ?? []) {
    const date = line.slice(1, 11);
    if (!bare.startsWith(date)) continue;
    const there = new Set(line.match(/[\p{L}\p{N}]+/gu) ?? []);
    if (words.every((w) => there.has(w))) return true;
  }
  return false;
}
