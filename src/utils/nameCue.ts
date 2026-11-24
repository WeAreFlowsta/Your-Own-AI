/**
 * The person's name, said in plain words ("my name is Marta", "call me
 * Sam"). No model reads the message: the name is taken only from these
 * fixed forms, so every AI goes on addressing the person by name.
 */
const FORMS = [
  /\bmy name is\s+([\p{Lu}][\p{L}'’-]{1,30}(?:\s+[\p{Lu}][\p{L}'’-]{1,30})?)/u,
  /\bmy name's\s+([\p{Lu}][\p{L}'’-]{1,30}(?:\s+[\p{Lu}][\p{L}'’-]{1,30})?)/u,
  /\b[Cc]all me\s+([\p{Lu}][\p{L}'’-]{1,30})\b/u,
];

/** The name a message states, or null. A question states nothing
 *  ("what if my name is on the list?"). */
export function nameFromCue(text: string): string | null {
  const t = text.trim();
  if (!t || t.length > 600) return null;
  for (const form of FORMS) {
    const m = form.exec(t);
    if (!m) continue;
    const sentence = t.slice(0, m.index + m[0].length + 1);
    const start = Math.max(sentence.lastIndexOf(". "), sentence.lastIndexOf("! "), sentence.lastIndexOf("\n")) + 1;
    const rest = t.slice(m.index + m[0].length);
    const end = rest.search(/[.!?\n]/);
    const closes = end < 0 ? "" : rest[end];
    if (closes === "?" || /\b(if|whether|suppose)\b/i.test(t.slice(start, m.index))) continue;
    return m[1].trim();
  }
  return null;
}
