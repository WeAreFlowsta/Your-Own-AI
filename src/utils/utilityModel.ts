/**
 * The on-device generative UTILITY MODEL — a small model (Ministral-3B) that
 * runs fact extraction + report/code mode classification on its own CPU server
 * (Rust `utility_chat`, port 8092), separate from the chat model. OPTIONAL: it's
 * a Settings → Components download. When absent, callers ride the active
 * chat/online model instead (the `fallbackModel`).
 *
 * The optional on-device utility model shared by extraction features.
 */
import { invoke } from "@tauri-apps/api/core";
import { modelManager } from "./modelManager";
import { llamaServerApi } from "./llamaServerApi";
import { UTILITY_MODEL } from "../data/recommended-models";
import type { ChatMessage } from "../types";

/** Is the optional on-device utility model installed? (Cheap FS check.) */
export async function isUtilityModelReady(): Promise<boolean> {
  try {
    return await modelManager.isModelDownloaded(UTILITY_MODEL.filename);
  } catch {
    return false;
  }
}

/** One-shot generation on the dedicated utility server (Rust). */
async function utilityChat(
  system: string,
  user: string,
  grammar: string | undefined,
  maxTokens: number,
): Promise<string> {
  return await invoke<string>("utility_chat", {
    model: UTILITY_MODEL.filename,
    system,
    user,
    grammar: grammar ?? null,
    maxTokens,
  });
}

/**
 * Run a small generative utility task (fact extraction / mode classification).
 * Prefers the on-device utility model when installed; otherwise rides
 * `fallbackModel` via the normal chat/online path (`llamaServerApi`). The
 * caller owns the offline/online wall by choosing `fallbackModel` — for an
 * offline AI it's the loaded local model, never an online one.
 *
 * `timeoutMs` guards the blocking classifier path (a cold utility server can
 * take seconds to load on first use). Returns the raw text; the caller parses.
 */
export async function runUtilityTask(
  system: string,
  user: string,
  grammar: string | undefined,
  maxTokens: number,
  fallbackModel: string | undefined,
  timeoutMs?: number,
  /** Skip the utility server and run on the (already warm) chat server -
   *  for quality-sensitive prose tasks where a stronger LOADED local model
   *  beats Ministral. Callers must only set this with a loaded model. */
  preferFallback?: boolean,
): Promise<string> {
  const work = (async () => {
    // Use the dedicated utility server only when the utility model is installed
    // AND it isn't already the loaded chat model. If the user is chatting WITH
    // it (fallbackModel is the utility filename), reuse that warm — often GPU —
    // instance via the fallback path instead of loading a second CPU copy.
    if (!preferFallback && fallbackModel !== UTILITY_MODEL.filename && (await isUtilityModelReady())) {
      return utilityChat(system, user, grammar, maxTokens);
    }
    const msgs: ChatMessage[] = [{ role: "user", content: user } as ChatMessage];
    let out = "";
    for await (const chunk of llamaServerApi.chatCompletion(
      msgs,
      system,
      undefined,
      maxTokens,
      false,
      fallbackModel,
      grammar,
    )) {
      if (chunk.type === "text") out += chunk.content;
    }
    return out;
  })();

  if (!timeoutMs) return work;
  return Promise.race([
    work,
    new Promise<string>((_, reject) =>
      setTimeout(() => reject(new Error("utility task timed out")), timeoutMs),
    ),
  ]);
}
