/**
 * Source-grounding — anchor each factual claim in an AI answer to the exact
 * verbatim quote (+ character span) in an attached document. The verifiable
 * evidence chain we record: claim → quote + offset → document SHA-256 → DHT entry.
 *
 * Reuses the grammar-constrained local-model pattern from memoryExtraction.ts:
 * a GBNF grammar forces clean JSON, a local model does the work, it's
 * fire-and-forget after a reply, and nothing leaves the device. Runs only when a
 * text document was attached to the turn.
 */
import type { ChatMessage } from "../types";
import { llamaServerApi } from "./llamaServerApi";

const MAX_GROUNDING_TOKENS = 768;

/** A grounded source recorded in a message's provenance (typed alongside web
 *  sources). `document` carries the verifiable quote + span; `image` is a coarse
 *  hash link (vision has no char offset). */
export interface GroundedSource {
  kind: "document" | "image";
  doc_sha256: string;
  doc_name?: string;
  claim?: string;
  quote?: string;
  span?: [number, number] | null;
}

// Forces the model to emit ONLY a JSON array of {claim, quote} (or []).
const GROUNDING_GRAMMAR = `root ::= "[" ws ( item ( ws "," ws item )* ws )? "]"
item ::= "{" ws qt "claim" qt ws ":" ws str ws "," ws qt "quote" qt ws ":" ws str ws "}"
qt ::= ["]
str ::= qt [^"]* qt
ws ::= [ \\t\\n\\r]*`;

const GROUNDING_SYSTEM_PROMPT = `You fact-check an AI's answer against a source document.
Given the SOURCE DOCUMENT and the AI's ANSWER, list the factual claims in the answer that the document supports, each paired with the EXACT verbatim quote from the document that backs it.
Rules:
- Focus on the MOST IMPORTANT claims — at most 6. Do not pad the list.
- Copy the quote CHARACTER-FOR-CHARACTER from the document so it can be located in it. Never paraphrase, summarise, translate, or fix the quote.
- Keep each quote SHORT — a single sentence or phrase, under ~25 words. Never quote a whole paragraph.
- Only include claims the document actually supports. Skip general knowledge, the AI's own opinions/commentary, and anything not in the document.
- If the answer makes no document-backed factual claims, return [].
Output ONLY a JSON array of {"claim":"...","quote":"..."} objects — nothing else.`;

interface ClaimQuote {
  claim: string;
  quote: string;
}

// Each complete object, scanned individually so a TRUNCATED array (model hit the
// token cap mid-write — common on small models) still yields every pair before the
// cutoff. Safe because the grammar's strings exclude `"`, so `[^"]*` can't run past
// a value boundary. Tolerates whitespace/newlines and any wrapping text.
const CLAIM_QUOTE_RE =
  /\{\s*"claim"\s*:\s*"([^"]*)"\s*,\s*"quote"\s*:\s*"([^"]*)"\s*\}/g;

/** Recover every complete {claim, quote} object from the model's output, even if
 *  the surrounding array was never closed. */
function parseClaimQuotes(text: string): ClaimQuote[] {
  const out: ClaimQuote[] = [];
  for (let m = CLAIM_QUOTE_RE.exec(text); m !== null; m = CLAIM_QUOTE_RE.exec(text)) {
    const claim = m[1].trim();
    const quote = m[2].trim();
    if (quote.length > 0) out.push({ claim, quote });
  }
  CLAIM_QUOTE_RE.lastIndex = 0;
  return out;
}

/** Exact char span of a verbatim quote in the source, or null (model
 *  paraphrased / whitespace differs). The quote is still recorded — it's
 *  verifiable by content even without an offset. */
function locateSpan(documentText: string, quote: string): [number, number] | null {
  const q = quote.trim();
  if (!q) return null;
  const i = documentText.indexOf(q);
  return i === -1 ? null : [i, i + q.length];
}

/** Ground an answer against an attached document. Returns one GroundedSource per
 *  supported claim (empty on no claims / failure — never throws). */
export async function groundDocument(params: {
  documentText: string;
  answerText: string;
  docSha256: string;
  docName?: string;
  model: string;
}): Promise<GroundedSource[]> {
  const { documentText, answerText, docSha256, docName, model } = params;
  if (!documentText.trim() || !answerText.trim() || !model) return [];
  try {
    const messages: ChatMessage[] = [
      {
        role: "user",
        content: `SOURCE DOCUMENT:\n${documentText}\n\nAI'S ANSWER:\n${answerText}`,
      },
    ];
    let out = "";
    for await (const chunk of llamaServerApi.chatCompletion(
      messages,
      GROUNDING_SYSTEM_PROMPT,
      undefined,
      MAX_GROUNDING_TOKENS,
      false,
      model,
      GROUNDING_GRAMMAR,
    )) {
      if (chunk.type === "text") out += chunk.content;
    }
    return parseClaimQuotes(out).map((p) => ({
      kind: "document" as const,
      doc_sha256: docSha256,
      doc_name: docName,
      claim: p.claim,
      quote: p.quote,
      span: locateSpan(documentText, p.quote),
    }));
  } catch (e) {
    console.warn("[Grounding] failed:", e);
    return [];
  }
}
