/**
 * The person's turn as it is sent to a model: what rides ahead of the
 * question for this turn only (an attached file, web pages that were read,
 * passages from their documents, what memory found), the question exactly
 * as typed, and memory's state note after it. One place builds it, so the
 * order is the same on every path. None of it is stored: the conversation
 * keeps the question as typed.
 *
 * The order is measured (answers were read with the memory block right
 * before the question and the state note right after it). The
 * `[User question]` marker stands once, and only when a file, pages or
 * document passages are present: the measured form with the memory block
 * alone has none.
 */
export interface UserTurnParts {
  file?: string | null;
  pages?: string | null;
  docs?: string | null;
  /** Memory's block: what a search of their conversations found. */
  before?: string | null;
  question: string;
  /** Memory's state note. */
  after?: string | null;
}

export function assembleUserTurn(p: UserTurnParts): string {
  const has = (s?: string | null): s is string => !!s && s.trim().length > 0;
  const lead = [p.file, p.pages, p.docs].filter(has);
  const ahead = has(p.before) ? [...lead, p.before] : lead;
  const question = lead.length ? `[User question]\n${p.question}` : p.question;
  const body = ahead.length ? `${ahead.join("\n\n")}\n\n${question}` : question;
  return has(p.after) ? `${body}\n\n${p.after}` : body;
}
