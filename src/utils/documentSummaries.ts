/**
 * Library cards and the library portrait, written on this device.
 *
 * Every document in the library gets one short summary ("what this is"),
 * written by the helper model or, without it, by the local chat model that
 * is loaded - never by an online model, whatever the AI's setting: a
 * person's books and letters stay on their computer. Long documents are
 * read in sampled parts and the notes reduced to one card.
 *
 * From those cards, the library portrait: two or three sentences about the
 * person from what they write (documents tagged Mine) and what they keep.
 * Cached in localStorage under a fingerprint of the cards, rewritten only
 * when a card or a tag changes, and given to every AI in the memory block.
 *
 * One run at a time; safe to trigger from any surface.
 */
import { invoke } from "@tauri-apps/api/core";
import { runUtilityTask, isUtilityModelReady } from "./utilityModel";
import { corpusDocuments, corpusDocumentText, corpusSetSummary, type DocRecord } from "./corpus";

/** Characters per sampled part, sized for the helper's 4K window on CPU. */
const PART_CHARS = 3000;
/** Parts sampled across a long document (start, middle, end). */
const MAX_PARTS = 4;
/** Whole documents up to this size are read in one pass. */
const SINGLE_PASS_CHARS = 3500;
/** How much of a document is fetched for sampling. */
const FETCH_CHARS = 400_000;

const PART_SYSTEM = [
  "You read one part of a longer document. Reply with at most three",
  "sentences saying what this part covers: the subject, the people or",
  "things named, and the kind of writing it is (a story, a letter, a report,",
  "notes, code, a manual). No preamble, no list.",
].join(" ");

const CARD_TAIL = [
  "Reply with ONE paragraph of at most 70 words: what the document is,",
  "what it covers, and who it is for. If it is clearly the author's own",
  "personal writing (a journal, letters, drafts, first-person reflection),",
  "say so in one clause. Plain prose, third person, no preamble, no list,",
  "no quotes.",
].join(" ");
const SINGLE_SYSTEM = `You write a short library card for one document. ${CARD_TAIL}`;
const REDUCE_SYSTEM = `You write a short library card for one document from notes about its parts. ${CARD_TAIL}`;

const PORTRAIT_SYSTEM = [
  'You describe a person from the documents in their library. Reply with two',
  'or three sentences in third person ("This person ..."): what they write',
  "about and how, from the documents marked as their own writing; what they",
  "keep and care about, from the rest. Say only what the summaries support;",
  "never invent names, jobs or places. No preamble, no list.",
].join(" ");

const PORTRAIT_KEY = "libraryPortrait";

type Listener = (docId: string, summary: string) => void;
const listeners = new Set<Listener>();
let inflight: Promise<void> | null = null;

/** Hear each card as it is written (a list can refresh its rows). */
export function onDocumentSummary(cb: Listener): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** The model that writes on this device, or null when there is none. */
async function localWriter(): Promise<{ model?: string; preferLoaded: boolean } | null> {
  if (await isUtilityModelReady()) return { preferLoaded: false };
  try {
    const loaded = await invoke<string | null>("get_current_model");
    if (loaded && loaded.endsWith(".gguf")) return { model: loaded, preferLoaded: true };
  } catch {
    /* unknown loaded state */
  }
  return null;
}

function usable(text: string): string {
  const t = text.trim().replace(/^["'\s]+|["'\s]+$/g, "");
  if (t.length < 20 || t.length > 700 || t.startsWith("-")) return "";
  return t;
}

/** Evenly spaced parts across a long text, cut at whitespace. */
export function sampleParts(text: string, partChars = PART_CHARS, maxParts = MAX_PARTS): string[] {
  const len = text.length;
  if (len <= partChars * 1.2) return [text];
  const n = Math.min(maxParts, Math.ceil(len / partChars));
  const parts: string[] = [];
  for (let i = 0; i < n; i++) {
    let start = Math.floor((i * (len - partChars)) / (n - 1));
    let end = start + partChars;
    if (i > 0) {
      const ws = text.indexOf(" ", start);
      if (ws !== -1 && ws - start < 200) start = ws + 1;
    }
    if (i < n - 1) {
      const ws = text.lastIndexOf(" ", end);
      if (ws !== -1 && end - ws < 200) end = ws;
    }
    parts.push(text.slice(start, end));
  }
  return parts;
}

async function writeCard(doc: DocRecord, writer: { model?: string; preferLoaded: boolean }): Promise<string> {
  const { text } = await corpusDocumentText(doc.doc_id, FETCH_CHARS);
  const head = `Document: "${doc.meta.title || doc.meta.filename}"${doc.meta.author ? ` by ${doc.meta.author}` : ""}`;
  const run = (system: string, user: string, maxTokens: number) =>
    runUtilityTask(system, user, undefined, maxTokens, writer.model, 120_000, writer.preferLoaded);
  if (text.length <= SINGLE_PASS_CHARS) {
    return usable(await run(SINGLE_SYSTEM, `${head}\n\n${text}`, 160));
  }
  const notes: string[] = [];
  for (const [i, part] of sampleParts(text).entries()) {
    const note = (await run(PART_SYSTEM, `${head} (part ${i + 1})\n\n${part}`, 120)).trim();
    if (note) notes.push(`Part ${i + 1}: ${note}`);
  }
  if (notes.length === 0) return "";
  return usable(await run(REDUCE_SYSTEM, `${head}\n\nNotes about its parts:\n${notes.join("\n")}`, 160));
}

/** Write a card for every document that has none. Runs once at a time;
 *  a second call joins the run in progress. */
export function summarizePendingDocuments(): Promise<void> {
  if (inflight) return inflight;
  inflight = (async () => {
    try {
      const pending = (await corpusDocuments()).filter((d) => !d.meta.summary);
      if (pending.length === 0) return;
      const writer = await localWriter();
      if (!writer) return; // no model on this device yet: the next trigger tries again
      let written = 0;
      for (const doc of pending) {
        try {
          const card = await writeCard(doc, writer);
          if (!card) continue;
          await corpusSetSummary(doc.doc_id, card);
          written++;
          for (const cb of listeners) cb(doc.doc_id, card);
        } catch (e) {
          console.warn(`[Library] card failed for ${doc.meta.filename}:`, e);
        }
      }
      if (written > 0) console.log(`[Library] ${written} document card(s) written on this device`);
    } catch (e) {
      console.warn("[Library] card pass skipped:", e);
    } finally {
      inflight = null;
    }
  })();
  return inflight;
}

function hash(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

/** The cached portrait, for the memory block and the memory page. */
export function getLibraryPortrait(): string {
  try {
    const raw = localStorage.getItem(PORTRAIT_KEY);
    if (!raw) return "";
    const v = JSON.parse(raw) as { text?: string };
    return typeof v.text === "string" ? v.text : "";
  } catch {
    return "";
  }
}

/** Rewrite the portrait when the cards or tags changed. Returns the
 *  current text (possibly unchanged). */
export async function refreshLibraryPortrait(): Promise<string> {
  try {
    const docs = (await corpusDocuments()).filter((d) => d.meta.summary);
    if (docs.length === 0) {
      localStorage.removeItem(PORTRAIT_KEY);
      return "";
    }
    const fp = hash(
      docs
        .map((d) => `${d.doc_id}:${d.meta.mine ? 1 : 0}:${hash(d.meta.summary || "")}`)
        .sort()
        .join("|"),
    );
    try {
      const raw = localStorage.getItem(PORTRAIT_KEY);
      if (raw) {
        const v = JSON.parse(raw) as { fp?: string; text?: string };
        if (v.fp === fp && v.text) return v.text;
      }
    } catch {
      /* rewrite */
    }
    const writer = await localWriter();
    if (!writer) return getLibraryPortrait();
    const line = (d: DocRecord) => `- ${d.meta.title || d.meta.filename}: ${d.meta.summary}`;
    const mine = docs.filter((d) => d.meta.mine).slice(0, 20).map(line);
    const kept = docs.filter((d) => !d.meta.mine).slice(0, 20).map(line);
    const user = [
      "Their own writing:",
      mine.length ? mine.join("\n") : "(none tagged yet)",
      "",
      "Kept in their library:",
      kept.length ? kept.join("\n") : "(nothing else)",
    ]
      .join("\n")
      .slice(0, 7000);
    const text = usable(
      await runUtilityTask(PORTRAIT_SYSTEM, user, undefined, 160, writer.model, 120_000, writer.preferLoaded),
    );
    if (!text) return getLibraryPortrait();
    localStorage.setItem(PORTRAIT_KEY, JSON.stringify({ fp, text, at: Date.now() }));
    console.log("[Library] portrait rewritten on this device");
    return text;
  } catch (e) {
    console.warn("[Library] portrait skipped:", e);
    return getLibraryPortrait();
  }
}
