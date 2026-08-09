/**
 * Episodic summaries for adopted history (import phase 2b, feature C).
 *
 * After an archive is adopted into an AI's conversations, this pass gives
 * that AI recall over them: one distilled summary per adopted conversation,
 * embedded into its episodic store with the conversation's hash and
 * ORIGINAL timestamp - so "remember when we discussed X" works across
 * history that predates the app.
 *
 * Idempotent by construction: it reads the chain (conversations whose
 * source is "import:*"), skips any conversation that already has an
 * episodic entry, and no-ops when the utility or embedding model is
 * missing - the next trigger (post-adoption, or the import card mounting)
 * picks up whatever remains. Summaries batch into ONE store write.
 */
import { getConversations, getTranscript } from "./holochainTranscripts";
import { runUtilityTask, isUtilityModelReady } from "./utilityModel";
import { isEmbeddingModelReady } from "./embeddings";
import { getAiMemories, addEpisodicSummaries } from "./transcriptMemory";

/** Transcript text handed to the summarizer per conversation. */
const MAX_TRANSCRIPT_CHARS = 4000;
const SUMMARY_SYSTEM = [
  "You summarize one past conversation between the user and an AI assistant.",
  "Reply with ONE sentence (at most 50 words) capturing what the user wanted",
  "or discussed and any concrete facts about them. Write in third person",
  '("The user ..."). No preamble, no quotes, no list.',
].join(" ");

let running = false;

/** Summarize any adopted conversations this AI can't yet recall. Safe to
 *  call repeatedly; runs at most once at a time. */
export async function summarizeAdoptedConversations(aiId: string): Promise<void> {
  if (!aiId || running) return;
  running = true;
  try {
    if (!(await isEmbeddingModelReady()) || !(await isUtilityModelReady())) return;

    const conversations = (await getConversations(aiId)).filter((c) =>
      c.source?.startsWith("import:"),
    );
    if (conversations.length === 0) return;

    const recalled = new Set(
      (await getAiMemories(aiId)).map((e) => e.conversation_hash),
    );
    const pending = conversations.filter((c) => !recalled.has(c.hash));
    if (pending.length === 0) return;

    const summaries: { conversationHash: string; text: string; createdAt: number }[] = [];
    for (const conv of pending) {
      try {
        const transcript = await getTranscript(conv.agent_key || aiId, conv.hash);
        const text = transcript
          .map((t) => `${t.role}: ${t.content}`)
          .join("\n")
          .slice(0, MAX_TRANSCRIPT_CHARS);
        if (!text.trim()) continue;
        const summary = (
          await runUtilityTask(SUMMARY_SYSTEM, text, undefined, 120, undefined, 30000)
        ).trim();
        if (summary) {
          summaries.push({
            conversationHash: conv.hash,
            text: summary,
            createdAt: conv.started_at,
          });
        }
      } catch (e) {
        console.warn("[Import] summary failed for one conversation:", e);
      }
    }

    const added = await addEpisodicSummaries(aiId, summaries);
    if (added > 0) {
      console.log(
        `[Import] ${added} adopted conversation(s) summarized into recall for AI ${aiId.slice(0, 8)}`,
      );
    }
  } finally {
    running = false;
  }
}
