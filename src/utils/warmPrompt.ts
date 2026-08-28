/**
 * Warm the loaded model with the selected AI's instructions so the first
 * real question pays only for its own words. The system prompt is built
 * exactly as a chat turn builds it (persona + the memory block), so the
 * server's prompt cache matches the turn's prefix; on a fresh install the
 * two are identical. One warm per (model, AI, memory) - never repeated.
 */
import { invoke } from "@tauri-apps/api/core";
import type { SelectedAiModel } from "../types";

const warmed = new Set<string>();

export async function warmSystemPrompt(ai: SelectedAiModel, model: string | null): Promise<void> {
  if (!model || model.startsWith("online:") || model.startsWith("external:")) return;
  try {
    const [{ buildSystemPrompt }, { loadMemoryBlock }] = await Promise.all([
      import("../hooks/useChat"),
      import("../utils/memory"),
    ]);
    const memory = await loadMemoryBlock("", { aiId: ai.id, conversationHash: null, queryVec: Promise.resolve(null) }).catch(() => "");
    const disposition = ai.aiConfig.lengthDisposition || "conversational";
    const system = buildSystemPrompt(ai, disposition, ai.aiConfig.defaultMode || "chat", memory, false);
    const key = `${model}|${ai.id}|${system.length}`;
    if (warmed.has(key)) return;
    warmed.add(key);
    const tokens = await invoke<number>("warm_chat_prompt", { system });
    console.log(`[warm] ${ai.label}: ${tokens} tokens of instructions ready on ${model}`);
  } catch (e) {
    console.warn("[warm] skipped", e);
  }
}
