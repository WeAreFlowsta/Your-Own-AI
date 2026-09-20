/**
 * Where a reply that the person interrupted may stop: the end of the
 * sentence being written, not mid-word. Inside a code block the unit is the
 * line. A cap keeps the wait short when no boundary comes.
 */

/** About 40 tokens at ~4 characters a token. */
export const BOUNDARY_CAP_CHARS = 160;

/** True when `text` ends inside an open ``` code block. */
export function insideCodeBlock(text: string): boolean {
  const fences = text.match(/^\s*```/gm);
  return !!fences && fences.length % 2 === 1;
}

/**
 * Where to cut a reply the person interrupted, or null to keep streaming.
 * `text` is the visible reply so far; `from` is its length at the moment the
 * person interrupted. The index is just past the sentence's punctuation (or
 * the line end), so a chunk that ran on into the next sentence is trimmed.
 */
export function boundaryIndex(text: string, from: number): number | null {
  const start = Math.min(from, text.length);
  const inCode = insideCodeBlock(text.slice(0, start));
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (c === "\n") return i;
    if (inCode) continue;
    if (c !== "." && c !== "!" && c !== "?") continue;
    // Sentence punctuation counts when a space follows (so "3.14" does
    // not), optionally after a closing quote, bracket or emphasis mark.
    let j = i + 1;
    while (j < text.length && `"')]*_`.includes(text[j])) j++;
    if (j < text.length && text[j] === " ") return j;
  }
  return text.length - start >= BOUNDARY_CAP_CHARS ? text.length : null;
}

export function reachedBoundary(text: string, from: number): boolean {
  return boundaryIndex(text, from) !== null;
}
