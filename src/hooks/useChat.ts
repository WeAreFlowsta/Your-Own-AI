/**
 * useChat Hook (Qwik)
 *
 * Manages chat state and interactions with local AI models.
 * Handles streaming responses and personality system prompts.
 *
 * In Qwik, state is held in a useStore and actions are $() QRLs.
 * flushSync is not needed — Qwik signals auto-update the DOM.
 */

import { useStore, useSignal, $, noSerialize, type Signal, type NoSerialize } from "@builder.io/qwik";
import type { Message, ChatMessage, SelectedAiModel, ChatAction, TurnMode } from "../types";
import { v4 as uuidv4 } from "uuid";
import {
  startConversation,
  recordMessage,
  recordGroundingAnnotation,
} from "../utils/holochainTranscripts";
import { extractAndStoreFacts, looksLikeUserFact } from "../utils/memoryExtraction";
import { isUtilityModelReady } from "../utils/utilityModel";
import { groundDocument, type GroundedSource } from "../utils/grounding";
import { loadMemoryBlock } from "../utils/memory";
import { indexTurn } from "../utils/transcriptMemory";
import { llamaServerApi } from "../utils/llamaServerApi";
import {
  getDispositionPrompt,
  getModePrompt,
  getTurnModeFromAction,
} from "../utils/responseLengthPrompts";
import { classifyTurnMode, codeKeywordHint } from "../utils/turnModeClassifier";
import { resolveRoutingSignals, type RoutingTask } from "../utils/routingTaskClassifier";
import { responseLengthOptions } from "../data/response-lengths";
import { getArchetypeById } from "../data/bundled-archetypes";
import { MISSION_CORE } from "../data/missionCore";
import { extractOnlineError } from "../utils/onlineErrors";
import {
  getMedicalModel,
  isOnlineMedicalChoice,
  medicalOnlineAlways,
} from "../utils/medicalModel";

/** Hex SHA-256 of a UTF-8 string (provenance for attached context). */
async function sha256Hex(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

// Keep the whole encrypted entry under the 1 MiB cipher cap: record the full
// attached content when small, otherwise just its hash + byte size.
const MAX_ATTACHMENT_CONTENT_BYTES = 600 * 1024;
async function buildAttachmentInfo(fileContext: string) {
  const bytes = new TextEncoder().encode(fileContext).length;
  return {
    bytes,
    sha256: await sha256Hex(fileContext),
    content: bytes <= MAX_ATTACHMENT_CONTENT_BYTES ? fileContext : undefined,
  };
}

// Record what a vision model SAW: the image's hash (provenance anchor that grounded
// claims point back to) + its size/mime, always; the data URL only when small, so a
// reloaded conversation can still show the thumbnail. ~256 KB keeps the user-message
// entry well under the 1 MiB cipher cap even alongside a text attachment.
const MAX_IMAGE_RECORD_BYTES = 256 * 1024;
async function buildImageAttachmentInfo(dataUrl: string, index: number) {
  const match = /^data:([^;]+);base64,(.*)$/s.exec(dataUrl);
  const mime = match?.[1] ?? "image/png";
  const raw = Uint8Array.from(atob(match?.[2] ?? ""), (c) => c.charCodeAt(0));
  const digest = await crypto.subtle.digest("SHA-256", raw);
  const sha256 = Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  const ext = (mime.split("/")[1] || "png").replace("jpeg", "jpg");
  return {
    filename: `image-${index + 1}.${ext}`,
    mime,
    bytes: raw.length,
    sha256,
    content: dataUrl.length <= MAX_IMAGE_RECORD_BYTES ? dataUrl : undefined,
  };
}

// Does a follow-up (with no image attached this turn) actually refer back to an
// earlier image? Deliberately specific — image nouns, visual attributes, and
// strong visual verbs — so generic follow-ups ("tell me more", "summarise that")
// fall through to the preferred text model (which still has the assistant's earlier
// description in context). Erring toward keeping vision is the mild failure; sending
// a "look at the corner" question to a blind model is the jarring one.
const VISION_REFERENCE_RE =
  /\b(image|images|picture|pictures|pic|pics|photo|photos|screenshot|screenshots|diagram|chart|graph|figure|drawing|logo|icon|scan|visual|colou?r|colou?rs|corner|background|foreground|shown|displayed|depicted|pictured|visible)\b/i;

/**
 * Extract thinking text from response content.
 * Handles <thinking>, <think>, and <|channel|> tags.
 */
/** Any marker the full parser would act on. One cheap native scan - when
 *  absent (ordinary chat replies), the streaming loop can skip the whole
 *  multi-regex parse and use the raw text as-is. */
const PARSE_MARKERS_RE = /<think|<\|channel\||<\|output\||<\/?report>|<\/?response/i;

function extractThinkingFromContent(content: string): {
  thinking: string;
  contentWithoutThinking: string;
} {
  const hasChannelTags = content.includes("<|channel|>");

  if (hasChannelTags) {
    const channelAnalysisMatch = content.match(
      /<\|channel\|>analysis<\|message\|>([\s\S]*?)(?:<\|end\|>|<\|start\|>|$)/i
    );
    const channelFinalMatch = content.match(
      /<\|channel\|>final<\|message\|>([\s\S]*?)$/i
    );

    let thinking = channelAnalysisMatch
      ? channelAnalysisMatch[1].trim()
      : "";
    let contentWithoutThinking = channelFinalMatch
      ? channelFinalMatch[1].trim()
      : "";

    if (contentWithoutThinking) {
      contentWithoutThinking = contentWithoutThinking
        .replace(/<think(?:ing)?>[\s\S]*?<\/think(?:ing)?>/gi, "")
        .trim();
    }

    return { thinking, contentWithoutThinking };
  }

  const thinkingRegex = /<think(?:ing)?[^>]*>([\s\S]*?)<\/think(?:ing)?>/gi;
  let thinking = "";

  let match;
  const matches: string[] = [];
  while ((match = thinkingRegex.exec(content)) !== null) {
    matches.push(match[1].trim());
  }

  if (matches.length > 0) {
    thinking = matches.join("\n\n");
  }

  const hasUnclosedThinkTag =
    /<think(?:ing)?[^>]*>/i.test(content) &&
    !/<\/think(?:ing)?>/i.test(
      content.substring(content.lastIndexOf("<think"))
    );

  if (hasUnclosedThinkTag) {
    const openTagMatch = content.match(/<think(?:ing)?[^>]*>([\s\S]*?)$/i);
    if (openTagMatch) {
      thinking = openTagMatch[1].trim();
    }
    return { thinking, contentWithoutThinking: "" };
  }

  let contentWithoutThinking = content.replace(thinkingRegex, "").trim();

  contentWithoutThinking = contentWithoutThinking
    .replace(/<\/?report>/gi, "")
    .replace(/<\|output\|>/gi, "")
    .replace(/<\/?response[^>]*>/gi, "")
    .trim();

  if (
    /<think(?:ing)?>/i.test(contentWithoutThinking) &&
    !/<\/think(?:ing)?>/i.test(contentWithoutThinking)
  ) {
    contentWithoutThinking = contentWithoutThinking
      .replace(/<think(?:ing)?>.*/is, "")
      .trim();
  }

  contentWithoutThinking = contentWithoutThinking
    .replace(/<\/?think(?:ing)?[^>]*$/i, "")
    .replace(/<\/?report[^>]*$/i, "")
    .replace(/<\/?response[^>]*$/i, "")
    .replace(/<\|output\|[^>]*$/i, "")
    .replace(/<[^>]*$/i, "")
    .trim();

  return { thinking, contentWithoutThinking };
}

function buildSystemPrompt(
  ai: SelectedAiModel,
  disposition: string,
  turnMode: TurnMode = "chat",
  userMemory: string = "",
  hasImages: boolean = false
): string {
  // Assemble the shared mission-core + the AI's CURRENT persona template, looked
  // up live by personality rather than a copy baked at creation — so changing the
  // personality (or us improving a persona) applies immediately, with no
  // re-create. Core first so the character is the most recent thing before the
  // reply. If the archetype is somehow missing, fall back to the stored prompt,
  // which is already the materialized core+persona (kept current by
  // republishStoredPersonas, which the inference API also relies on).
  const archetype = getArchetypeById(ai.aiConfig.baseArchetypeId);
  let systemPrompt = archetype
    ? `${MISSION_CORE}\n\n${archetype.systemPromptTemplate}`
    : ai.aiConfig.systemPrompt || "";

  // Phase A memory: the user profile is injected into the {{userNameInfo}}
  // slot (it sits in the prompt's "User Information" block). Empty memory
  // falls back to the original name-unknown behaviour.
  systemPrompt = systemPrompt.replace(
    /\{\{userNameInfo\}\}/g,
    userMemory.trim()
      ? userMemory
      : "Their name is unknown. Do not assume or allude to it."
  );

  const dispositionPrompt = getDispositionPrompt(disposition);
  systemPrompt = systemPrompt.replace(
    /\{\{aiResponseLength\}\}/g,
    dispositionPrompt
  );

  systemPrompt = systemPrompt.replace(
    /\{\{aiName\}\}/g,
    ai.aiConfig.name
  );

  // Always include lightweight code formatting guidance
  systemPrompt +=
    "\n\nWhen including code in your response, always use markdown code blocks with the language identifier (e.g. ```python, ```javascript, ```html). Always close code blocks with ```.";

  // Vision: small models reflexively disclaim ("I'm a text-only language model, I
  // can't see images") even when an image is attached through their vision
  // capability — then describe it anyway. This counters that false disclaimer.
  if (hasImages) {
    systemPrompt +=
      "\n\nThe user has attached one or more images to this message, and you can see them directly through your vision capability. Look at the image and answer about what is actually in it. Never say that you cannot see images or that you are a text-only model — you can see this one.";
  }

  // Per-turn mode block (report ceremony / code-first) — decoupled from length.
  systemPrompt += getModePrompt(turnMode);

  const useEmojis = ai.aiConfig.useEmojis ?? false;
  if (useEmojis) {
    systemPrompt +=
      " It is a requirement that you generously use relevant emojis in this response to give it a friendly and expressive tone. Aim to include multiple emojis where they fit naturally within the text.";
  } else {
    systemPrompt += " You must not use any emojis in your response.";
  }

  return systemPrompt;
}

export interface UseChatState {
  messages: Message[];
  isLoading: boolean;
  error: string | null;
  /** A turn parked because the user must resolve something before it sends —
   *  pick/download a vision model, or consent to sending an attachment to the
   *  cloud. The inline card (chat route) resends it once resolved. */
  pendingTurn: {
    userInput: string;
    chatAction: ChatAction;
    images: string[];
    fileContext?: string;
    visionModel: string; // the vision model to switch to ("" if none / N/A)
  } | null;
  /** Current Holochain conversation hash (null if not recording) */
  conversationHash: string | null;
  /** Opaque per-conversation key sent with online turns as the provider's
   *  prompt-cache routing key (fresh per new chat; the conversation hash
   *  when an existing conversation is opened, so resuming it stays warm). */
  cacheKey: string;
  /** Message sequence counter for ordering */
  messageSequence: number;
}

/** Random opaque id for prompt-cache routing - no user or device meaning. */
export function newCacheKey(): string {
  return `c-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export interface UseChatProps {
  selectedAi: Signal<SelectedAiModel>;
  currentModel: Signal<string | null>;
  isModelLoading: Signal<boolean>;
  modelLoadTime: Signal<number | null>;
  modelTooBig: Signal<boolean>;
}

export function useChat(props: UseChatProps) {
  const state = useStore<UseChatState>({
    messages: [],
    isLoading: false,
    error: null,
    pendingTurn: null,
    conversationHash: null,
    cacheKey: newCacheKey(),
    messageSequence: 0,
  });

  // AbortController stored in a signal with noSerialize since it's not serializable
  const abortControllerRef = useSignal<NoSerialize<AbortController> | null>(null);

  const sendMessage = $(
    async (
      userInput: string,
      chatAction: ChatAction = null,
      fileContext?: string,
      images: string[] = [],
      modelOverride?: string,
      consentGranted: boolean = false,
    ) => {
      if (!userInput.trim() || state.isLoading) return;
      // Attached document names for the bubble's file chips - parsed from
      // the context's own "--- <name> ---" headers so the call signature
      // stays put. The text itself never enters the bubble.
      const attachedFileNames: string[] = fileContext
        ? [...fileContext.matchAll(/^--- (.+?) ---$/gm)].map((m) => m[1])
        : [];
      // Starting a turn clears any parked prompt (vision / consent).
      state.pendingTurn = null;

      const selectedAi = props.selectedAi.value;
      const currentModel = props.currentModel.value;

      // Two axes (see RESPONSE_LENGTH_AND_MODES.md):
      //  - disposition: the AI's resting length lean (per-AI, soft).
      //  - turnMode: what kind of answer THIS turn is. Explicit sparkle action
      //    wins; the classifier (Phase D) will slot in where actionMode is null.
      const disposition = selectedAi.aiConfig.lengthDisposition || "conversational";
      const actionMode = getTurnModeFromAction(chatAction);
      // Provisional mode for the optimistic bubble below (shown the instant Enter is
      // pressed). The classifier refines it AFTER the bubble is on screen — so Enter
      // never waits on classification.
      const provisionalMode: TurnMode =
        actionMode || selectedAi.aiConfig.defaultMode || "chat";

      // Vision is needed if THIS turn has an image, or a recent turn had one AND
      // this message actually refers back to it. Without the reference check, the
      // whole conversation would stick on the (often weaker) vision model once any
      // image appeared — so a plain "now write me code" follow-up wrongly skipped
      // the user's preferred text model. The image stays in the transcript and the
      // assistant's earlier description stays in context, so generic follow-ups are
      // still answered well by the text model.
      const recentImage = state.messages
        .slice(-8)
        .some((m) => m.images && m.images.length > 0);
      const needsVision =
        images.length > 0 ||
        (recentImage && VISION_REFERENCE_RE.test(userInput));

      // Build chat history from existing messages BEFORE adding the new ones
      // (avoids duplicating the user message). A past user turn that had images is
      // rebuilt as a multimodal content array so the model can still see them.
      const chatHistory: ChatMessage[] = state.messages
        .filter((m) => !m.isLoading && !m.error)
        .map((m) =>
          m.images && m.images.length > 0
            ? {
                role: m.role,
                content: [
                  { type: "text" as const, text: m.content },
                  ...m.images.map((url) => ({
                    type: "image_url" as const,
                    image_url: { url },
                  })),
                ],
              }
            : { role: m.role, content: m.content },
        );

      // Add current user input to history (with file context for AI, if provided).
      // A vision turn becomes a multimodal content array: the text plus each
      // attached image as a data-URL image_url part.
      const historyContent = fileContext
        ? `${fileContext}\n\n[User question]\n${userInput}`
        : userInput;
      chatHistory.push(
        images.length > 0
          ? {
              role: "user",
              content: [
                { type: "text", text: historyContent },
                ...images.map((url) => ({
                  type: "image_url" as const,
                  image_url: { url },
                })),
              ],
            }
          : { role: "user", content: historyContent },
      );

      // llama.cpp's multimodal path CRASHES on more than one image in a single
      // request, so keep only the single most-recent image across the whole
      // conversation and strip the rest back to text. (A follow-up references the
      // latest image; older ones stay as their text.) When vision ISN'T needed this
      // turn, strip ALL images so the history is clean text for the text model.
      let lastImageMsg = -1;
      if (needsVision) {
        for (let i = chatHistory.length - 1; i >= 0; i--) {
          if (Array.isArray(chatHistory[i].content)) {
            lastImageMsg = i;
            break;
          }
        }
      }
      chatHistory.forEach((msg, i) => {
        if (!Array.isArray(msg.content)) return;
        const text = msg.content
          .filter((p): p is { type: "text"; text: string } => p.type === "text")
          .map((p) => p.text)
          .join("\n");
        if (i !== lastImageMsg) {
          msg.content = text; // older image messages → text only
        } else {
          const firstImage = msg.content.find((p) => p.type === "image_url");
          msg.content = firstImage ? [{ type: "text", text }, firstImage] : text;
        }
      });

      // Optimistic UI: show the user's message and a pending assistant bubble
      // IMMEDIATELY — before routing or any model load — so pressing Enter flips
      // straight to the conversation view instead of sitting on the empty hero
      // for the few seconds it takes to resolve/load the model. The existing
      // model-loading indicator and the bubble's "thinking" state cover the wait.
      const userMessage: Message = {
        id: uuidv4(),
        role: "user",
        content: userInput,
        images: images.length > 0 ? images : undefined,
        attachedFiles: attachedFileNames.length > 0 ? attachedFileNames : undefined,
        model: "user",
      };
      const assistantId = uuidv4();
      const assistantMessage: Message = {
        id: assistantId,
        role: "assistant",
        content: "",
        model: selectedAi.id,
        isLoading: true,
        aiLabel: selectedAi.label,
        aiImageUrl: selectedAi.imageUrl || undefined,
        turnMode: provisionalMode,
        turnModeAuto: false,
      };
      state.messages = [...state.messages, userMessage, assistantMessage];
      state.isLoading = true;
      state.error = null;

      // Embed this turn's text ONCE, immediately - the router's semantic
      // freshness gate and memory retrieval both need the same vector, and
      // each used to compute it separately (two embed-server round trips).
      // Fails soft to null; both consumers degrade exactly as before.
      const queryVecPromise: Promise<number[] | null> = import(
        "../utils/embeddings"
      )
        .then(({ embedQuery }) => embedQuery(userInput))
        .catch(() => null);

      // Memory retrieval is independent of routing - start it NOW so its
      // latency hides behind classification/routing/model-load instead of
      // adding to them. Wasted work on the rare aborted turn (vision card,
      // consent) is fine: it's read-only.
      const memoryBlockPromise = loadMemoryBlock(userInput, {
        aiId: selectedAi.id,
        conversationHash: state.conversationHash,
        queryVec: queryVecPromise,
      }).catch(() => "");

      // Phase D: NOW that the bubble + action bar are on screen, classify the turn —
      // the classify call no longer makes Enter feel laggy (it's absorbed into the
      // loading state). With no explicit sparkle, it can upgrade chat → report/code
      // from the message (keyword-gated so casual chat skips it; never throws).
      // Kicked off NOW and joined just before the prompt is built: the
      // classifier used to gate routing serially, stacking its latency (and
      // its worst case - waiting on a loading local server) in front of
      // every turn, online ones included. It runs CONCURRENTLY with
      // vision/routing instead; nothing before the prompt needs its verdict.
      let classifyPromise: Promise<'report' | 'code' | null> = Promise.resolve(null);
      const smartMode =
        (typeof localStorage !== "undefined"
          ? localStorage.getItem("smartModeDetection")
          : null) !== "false"; // default ON
      if (!actionMode && smartMode) {
        const prevMode = [...state.messages]
          .reverse()
          .find((m) => m.role === "assistant" && !m.isLoading)?.turnMode;
        const loaded =
          currentModel && !currentModel.startsWith("auto:") ? currentModel : undefined;
        const classifyModel =
          loaded ||
          (selectedAi.aiConfig.model?.startsWith("online:") ||
          selectedAi.aiConfig.model?.startsWith("external:")
            ? selectedAi.aiConfig.model
            : undefined);
        classifyPromise = classifyTurnMode(userInput, classifyModel, prevMode);
      }

      // On a fatal pre-generation error (no model / load failed), drop the
      // optimistic bubbles and surface the full-screen banner as before.
      const abortWith = (errMessage: string) => {
        state.messages = state.messages.filter(
          (m) => m.id !== assistantId && m.id !== userMessage.id
        );
        state.isLoading = false;
        props.isModelLoading.value = false;
        state.error = errMessage;
      };

      // Resolve the model for THIS question (auto:* → concrete) via the backend
      // router — the single source of truth.
      // A modelOverride (the user accepted the "use a vision model for this image"
      // card) pins this turn to that model and skips routing.
      let preferredModel = modelOverride ?? selectedAi.aiConfig.model ?? null;
      // Routing receipt (shown on the Model button + recorded). Declared here
      // because the vision path below may set them before the routing block.
      let routedReason: string | undefined;
      let routedTask: string | undefined;
      // Auto modes may transparently switch an image turn to a vision model; a
      // pinned model is left as-is (we ask before switching — see vision block).
      const isAutoMode = !modelOverride && !!preferredModel?.startsWith("auto:");

      // Vision-aware routing: for an image turn in an Auto mode, vision capability
      // takes priority over the freshness route. Prefer a ready OFFLINE vision
      // model — private, silent, free. If none and the AI is Online+Offline (they
      // opted into cloud), route to a vision-capable ONLINE model (consent is asked
      // below). Offline-only with no offline vision falls through to the freshness
      // route, which lands on the download card.
      if (needsVision && isAutoMode) {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          // The pick is medical-aware: a health image (X-ray, scan, skin
          // photo) prefers MedGemma, and the reason carries the receipt line.
          // Pass the shared turn embedding so its medical gate is free.
          const qv = await queryVecPromise;
          const offlineVision = await invoke<{ model: string; reason: string } | null>(
            "find_vision_model",
            { query: userInput, queryVec: qv ?? undefined },
          );
          if (offlineVision) {
            preferredModel = offlineVision.model; // offline-first, silent
            routedReason = offlineVision.reason;
            routedTask = "vision";
            console.log(`[Vision] offline vision ${offlineVision.model} (${offlineVision.reason})`);
          } else {
            // No offline vision downloaded. A HEALTH image must still stay
            // local (the promise) - fall through to the download card rather
            // than offering a cloud model. Non-health images may go online in
            // online-offline mode (consent asked below).
            const medicalImage = await invoke<boolean>("is_medical_query", {
              query: userInput,
              queryVec: qv ?? undefined,
            }).catch(() => false);
            if (!medicalImage && preferredModel === "auto:online-offline") {
              const onlineModels = await invoke<{ id: string; vision?: boolean }[]>(
                "list_online_models",
              );
              const visionOnline = onlineModels.find((m) => m.vision === true);
              if (visionOnline) {
                preferredModel = visionOnline.id; // "online:…" → consent asked below
                console.log(`[Vision] no offline vision — routing image to online ${visionOnline.id}`);
              }
            } else if (medicalImage) {
              console.log("[Vision] health image, no offline vision — staying local (download card)");
            }
          }
        } catch (e) {
          console.warn("[Vision] vision-aware routing failed:", e);
        }
      }

      // Routing receipt for this turn (shown in the hover strip + recorded in
      // the transcript). Stays undefined when the user picked the model.
      if (!modelOverride && preferredModel?.startsWith("auto:")) {
        // An attached image or document is a privacy fact the router must
        // weigh: in online-offline mode it routes OFFLINE unless the user has
        // given standing consent for attachments to go online (Settings >
        // Routing > "Send attachments to online models"). With consent, the
        // router is free to pick online when that is better - and no
        // consent modal, since consent is already given. Without it, the
        // pick stays local instead of routing straight into a modal.
        const hasAttachment = images.length > 0 || !!fileContext;
        const attachmentsMayGoOnline =
          localStorage.getItem("allowAttachmentsOnline") === "true";
        const requestedMode =
          preferredModel === "auto:online-offline"
            ? "online-offline"
            : preferredModel === "auto:my-hardware"
              ? "my-hardware"
              : "offline";
        const mode =
          requestedMode === "online-offline" && hasAttachment && !attachmentsMayGoOnline
            ? "offline"
            : requestedMode;
        if (mode !== requestedMode) {
          console.log("[Router] attachment without online consent - routing offline");
        }
        // Eagerness (online FRESHNESS threshold) — Settings → Routing.
        const eagerness =
          localStorage.getItem("smartRoutingEagerness") || "balanced";
        // Offline model-pick lean (speed / balanced / quality) — Settings → Routing.
        const lean = localStorage.getItem("routingOfflineLean") || "balanced";
        // Per-slot online model choices — Settings → Routing ("" / absent =
        // the router's recommended default for that slot).
        const onlineFresh = localStorage.getItem("routingOnlineFresh") || undefined;
        const onlineHardCode = localStorage.getItem("routingOnlineHardCode") || undefined;
        const onlineHardGeneral = localStorage.getItem("routingOnlineHardGeneral") || undefined;
        // Routing task: the report/code classifier's CODE signal is a free base
        // hint; the dedicated routing-task classifier upgrades it ONLY when a
        // specialist model is installed (otherwise it's gated off — no cost).
        // See ROUTING_TASK_COMPLEXITY.md.
        // Keyword hint only - the classifier is still in flight (parallel).
        // The routing-task classifier upgrades/corrects this where it matters.
        const baseTask: RoutingTask = codeKeywordHint(userInput) ? "code" : "general";
        const routeClassifyModel =
          currentModel && !currentModel.startsWith("auto:") ? currentModel : undefined;
        // Difficulty classification only runs when its result could matter:
        // escalation is inherent to online-offline mode (the mode IS the
        // consent to go online when it helps), and only that mode escalates.
        const wantDifficulty = mode === "online-offline";
        const signals = await resolveRoutingSignals(
          userInput,
          routeClassifyModel,
          baseTask,
          wantDifficulty,
        );
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          // Only online-offline mode uses the vector (semantic freshness);
          // it was kicked off at send, so this await is usually instant.
          const queryVec =
            mode === "online-offline" ? await queryVecPromise : undefined;
          const r = await invoke<{ model: string; reason: string }>("route_model", {
            mode,
            query: userInput,
            task: signals.task,
            difficulty: signals.difficulty,
            eagerness,
            lean,
            onlineFresh,
            onlineHardCode,
            onlineHardGeneral,
            queryVec: queryVec ?? undefined,
            // The turn's size, so the pick can set aside models whose runtime
            // context is too short for it: history + this message + any
            // attached text (chars/4) + ~1000 tokens per image + persona room.
            turnTokens: Math.ceil(
              (state.messages.reduce((n, m) => n + (m.content?.length ?? 0), 0) +
                userInput.length +
                (fileContext?.length ?? 0) +
                4000) / 4,
            ) + images.length * 1000,
          });
          console.log(`[Router] ${mode} → ${r.model} (${r.reason})`);
          preferredModel = r.model;
          routedReason = r.reason;
          routedTask = signals.task;
        } catch (e) {
          console.warn("[Router] resolve failed:", e);
          // Leaves the auto: sentinel → surfaces as NO_MODELS_AVAILABLE below.
        }
      }

      // Health questions with an ONLINE preference (Settings > Routing):
      // the choice never fires silently - each health turn asks first, like
      // an attachment going online, unless the user chose "always". Only in
      // the auto modes or on an already-online AI: a hard-pinned offline AI
      // stays offline entirely (pinning is explicit). Declining answers on
      // the device via normal routing.
      if (!modelOverride && (isAutoMode || preferredModel?.startsWith("online:"))) {
        const medPref = getMedicalModel();
        if (isOnlineMedicalChoice(medPref)) {
          try {
            const { invoke } = await import("@tauri-apps/api/core");
            const qv = await queryVecPromise;
            const isMed = await invoke<boolean>("is_medical_query", {
              query: userInput,
              queryVec: qv ?? undefined,
            });
            if (isMed) {
              if (medicalOnlineAlways() && medPref) {
                preferredModel = medPref;
              } else {
                state.pendingTurn = {
                  userInput,
                  chatAction,
                  images,
                  fileContext,
                  visionModel: "",
                };
                abortWith(
                  JSON.stringify({
                    code: "medical_consent",
                    onlineModel: medPref,
                  }),
                );
                return;
              }
            }
          } catch (e) {
            console.warn("[Medical] online-choice check failed - staying offline:", e);
          }
        }
      }

      // Switch model if needed (online models live on the proxy, external
      // models on the user's own server — nothing to load locally either way).
      const isOnlineModel = !!preferredModel?.startsWith("online:");
      const isExternalModel = !!preferredModel?.startsWith("external:");
      if (!isOnlineModel && !isExternalModel && preferredModel && preferredModel !== currentModel) {
        console.log(
          "[useChat] Switching model:",
          currentModel,
          "->",
          preferredModel
        );
        props.isModelLoading.value = true;
        // This load belongs to THIS turn - say so on the bubble (the action
        // bar is turn-scoped; without a statusText it shows nothing).
        {
          const { formatModelDisplayName } = await import("../utils/modelNameFormatter");
          const loadingText = `Loading ${formatModelDisplayName(preferredModel)}...`;
          state.messages = state.messages.map((m) =>
            m.id === assistantId ? { ...m, statusText: loadingText } : m,
          );
        }
        try {
          props.modelLoadTime.value = Date.now();
          // Vision-capable models load with their projector already attached (the
          // backend pairs it whenever one exists), so an image turn never triggers a
          // mid-conversation reload. `withVision` is passed for completeness.
          // load_model blocks until the server reports healthy - no settle
          // delay needed (and the stream path health-checks again anyway).
          const { loadModelBounded } = await import("../utils/loadModelBounded");
          await loadModelBounded({ filename: preferredModel, withVision: needsVision, reason: "chat-switch" });
          props.currentModel.value = preferredModel;
          props.modelTooBig.value = false;
        } catch (error) {
          const errorMessage =
            error instanceof Error ? error.message : String(error);
          if (errorMessage.includes("MODEL_DEVICE_UNSUPPORTED")) {
            // Normally the backend retries on the processor and this never
            // surfaces - it reaches here only if that retry also failed.
            props.currentModel.value = preferredModel;
            props.modelTooBig.value = true;
            abortWith(
              `Your graphics card can't run AI models right now (the app will use your processor instead). The retry didn't complete - restarting the app usually finishes the switch.`
            );
          } else if (errorMessage.includes("MODEL_LOAD_CRASHED")) {
            props.currentModel.value = preferredModel;
            props.modelTooBig.value = true;
            abortWith(
              `The AI engine crashed while loading ${selectedAi.label}'s model. This looks like a problem on our side, not your hardware - restarting the app may help, and the log file helps us fix it.`
            );
          } else if (errorMessage.includes("MODEL_TOO_LARGE")) {
            // Show it red in the header (name visible, not loaded) and tell the user.
            props.currentModel.value = preferredModel;
            props.modelTooBig.value = true;
            abortWith(
              `${selectedAi.label}'s model is too large for your graphics card. Pick a smaller model on the Offline Models page (look for the "Your GPU" badge).`
            );
          } else if (errorMessage.includes("MODEL_LOAD_TIMEOUT")) {
            props.currentModel.value = preferredModel;
            props.modelTooBig.value = true;
            abortWith(
              `${selectedAi.label}'s model took too long to load on this hardware, so the attempt was stopped. It won't be retried this session - a smaller model will load quickly (look for the "Your GPU" badge on the Offline Models page).`
            );
          } else if (errorMessage.includes("MODEL_FILE_UNREADABLE")) {
            props.currentModel.value = preferredModel;
            props.modelTooBig.value = true;
            abortWith(
              `${selectedAi.label}'s model file couldn't be opened from disk. Downloading it again from the Offline Models page usually fixes this - if it keeps happening, the log file shows why.`
            );
          } else {
            abortWith(
              errorMessage.includes("Model file not found")
                ? "NO_MODELS_AVAILABLE"
                : `Failed to load model "${preferredModel}" for ${selectedAi.label}: ${errorMessage}`
            );
          }
          return;
        }
      }

      if (!currentModel && !preferredModel) {
        abortWith("NO_MODELS_AVAILABLE");
        return;
      }

      // Vision: an image turn needs the server to have a projector loaded.
      //  1. Reload the resolved model — pairs its own projector if it has one.
      //  2. Still can't see + Auto mode → transparently switch this turn to a
      //     downloaded vision model (Auto modes just do it).
      //  3. Still can't see → can't proceed (a pinned non-vision model, or no
      //     vision model downloaded). [2b: a pinned model will get an inline
      //     "switch?" card; no vision model → an inline download card.]
      // Online models handle vision on the proxy.
      if (needsVision && !isOnlineModel && preferredModel) {
        const { invoke } = await import("@tauri-apps/api/core");
        // Determine vision-readiness defensively. The gate below is OUTSIDE this
        // try so an image is NEVER silently sent to a model that can't see it,
        // even if a command errors (e.g. an old backend without find_vision_model).
        let visionReady = false;
        try {
          visionReady = await invoke<boolean>("is_vision_ready");
          console.log(
            `[Vision] image turn — model=${preferredModel} auto=${isAutoMode} ready=${visionReady}`,
          );
          // Turn-owned loads announce themselves on the bubble (the action
          // bar is turn-scoped and silent without a statusText).
          const sayLoading = async (filename: string) => {
            const { formatModelDisplayName } = await import("../utils/modelNameFormatter");
            const loadingText = `Loading ${formatModelDisplayName(filename)}...`;
            state.messages = state.messages.map((m) =>
              m.id === assistantId ? { ...m, statusText: loadingText } : m,
            );
          };
          if (!visionReady) {
            // Reload the resolved model to pair its own projector, if it has one.
            props.isModelLoading.value = true;
            await sayLoading(preferredModel!);
            const { loadModelBounded } = await import("../utils/loadModelBounded");
            await loadModelBounded({ filename: preferredModel, withVision: true, reason: "vision-reload" });
            props.currentModel.value = preferredModel;
            visionReady = await invoke<boolean>("is_vision_ready");
          }
          if (!visionReady && isAutoMode) {
            const qv = await queryVecPromise;
            const pick = await invoke<{ model: string; reason: string } | null>(
              "find_vision_model",
              { query: userInput, queryVec: qv ?? undefined },
            );
            console.log(`[Vision] auto-switch candidate: ${pick?.model ?? "none"}`);
            if (pick) {
              await sayLoading(pick.model);
              const { loadModelBounded } = await import("../utils/loadModelBounded");
              await loadModelBounded({ filename: pick.model, withVision: true, reason: "vision-auto-switch" });
              props.currentModel.value = pick.model;
              preferredModel = pick.model;
              routedReason = pick.reason;
              routedTask = "vision";
              visionReady = await invoke<boolean>("is_vision_ready");
            }
          }
        } catch (e) {
          console.warn("[Vision] readiness check failed:", e);
          visionReady = false;
        }
        if (!visionReady) {
          // Can't auto-resolve. Park the turn and surface an inline card: a
          // "switch?" card if a vision model is downloaded (pinned AI), or a
          // "download a vision model" card if none is. The card (chat route)
          // resends via state.pendingTurn.
          let visionModel: string | null = null;
          try {
            const qv = await queryVecPromise;
            const pick = await invoke<{ model: string; reason: string } | null>(
              "find_vision_model",
              { query: userInput, queryVec: qv ?? undefined },
            );
            visionModel = pick?.model ?? null;
          } catch {
            /* old backend / no command — treat as none */
          }
          state.pendingTurn = {
            userInput,
            chatAction,
            images,
            fileContext,
            visionModel: visionModel ?? "",
          };
          abortWith(
            JSON.stringify({
              code: "vision_needed",
              kind: visionModel ? "switch" : "download",
              aiLabel: selectedAi.label,
              visionModel: visionModel ?? "",
            }),
          );
          return;
        }
      }

      // Online vision: don't send an image to an online model that can't see it
      // (the grok-search 422). If it isn't vision-capable per the proxy catalog,
      // offer the offline vision path (or a download) via the same card. Checked
      // before consent — no point asking to send an image that won't be used.
      if (needsVision && isOnlineModel && preferredModel) {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const onlineModels = await invoke<{ id: string; vision?: boolean }[]>(
            "list_online_models",
          );
          const m = onlineModels.find((x) => x.id === preferredModel);
          if (m && m.vision === false) {
            let visionModel: string | null = null;
            try {
              const qv = await queryVecPromise;
              const pick = await invoke<{ model: string; reason: string } | null>(
                "find_vision_model",
                { query: userInput, queryVec: qv ?? undefined },
              );
              visionModel = pick?.model ?? null;
            } catch {
              /* none */
            }
            state.pendingTurn = {
              userInput,
              chatAction,
              images,
              fileContext,
              visionModel: visionModel ?? "",
            };
            abortWith(
              JSON.stringify({
                code: "vision_needed",
                kind: visionModel ? "switch" : "download",
                aiLabel: selectedAi.label,
                visionModel: visionModel ?? "",
              }),
            );
            return;
          }
        } catch (e) {
          console.warn("[Vision] online capability check failed:", e);
        }
      }

      // Cloud consent: an attachment (image OR document) sent to an online model
      // leaves the device. Ask first, unless the user chose to always allow.
      if (
        isOnlineModel &&
        (images.length > 0 || !!fileContext) &&
        !consentGranted &&
        localStorage.getItem("allowAttachmentsOnline") !== "true"
      ) {
        state.pendingTurn = {
          userInput,
          chatAction,
          images,
          fileContext,
          visionModel: "",
        };
        abortWith(
          JSON.stringify({
            code: "cloud_consent",
            onlineLabel: selectedAi.label,
            hasImage: images.length > 0,
            hasDoc: !!fileContext,
          }),
        );
        return;
      }

      // Cancel any ongoing request
      if (abortControllerRef.value) {
        abortControllerRef.value.abort();
      }
      const controller = new AbortController();
      abortControllerRef.value = noSerialize(controller);

      // Records: the conversation and the user's message are written the
      // moment they are sent - in parallel with the request, never gating
      // it - so the conversation exists in your records while the reply is
      // still streaming (it used to appear only after the reply finished)
      // and the user's words survive a stopped, failed, or crashed reply.
      // The assistant turn is recorded when the reply finishes, or, when
      // stopped, as far as it got (flagged stopped). Same order as agent
      // sessions.
      const recordCtx = {
        holochainId: selectedAi.aiConfig?.agentPubKey as string | undefined,
        modelName: (preferredModel || currentModel || "unknown") as string,
        turnMode: "chat" as string,
      };
      let preRecord: Promise<{
        userMsgHash: string | null;
        userAttachment: Awaited<ReturnType<typeof buildAttachmentInfo>> | undefined;
        imageInfos: Awaited<ReturnType<typeof buildImageAttachmentInfo>>[] | undefined;
        appVersion: string;
      } | null> | null = null;

      // A web-search model researches for a long stretch (often 30-60s)
      // before its first visible text - mark the turn so the status line can
      // say what's actually happening instead of a generic "thinking".
      try {
        const { getCachedModels } = await import("../utils/modelCache");
        const searchingWeb =
          !!preferredModel?.startsWith("online:") &&
          getCachedModels().online?.find((m) => m.id === preferredModel)?.category ===
            "web_search";
        if (searchingWeb) {
          state.messages = state.messages.map((m) =>
            m.id === assistantId ? { ...m, searchingWeb: true } : m,
          );
        }
      } catch {
        /* status nicety only */
      }

      // Join the parallel classifier: the mode first matters HERE (prompt,
      // max_tokens, thinking capture). Precedence: explicit sparkle →
      // classifier → the AI's default mode → chat.
      const classifierMode = await classifyPromise;
      const turnMode: TurnMode =
        actionMode || classifierMode || selectedAi.aiConfig.defaultMode || "chat";
      const turnModeAuto = !actionMode && !!classifierMode;
      // Reflect a classifier upgrade on the already-visible bubble.
      if (turnMode !== provisionalMode || turnModeAuto) {
        state.messages = state.messages.map((m) =>
          m.id === assistantId ? { ...m, turnMode, turnModeAuto } : m,
        );
      }

      try {
        // Memory: the block was kicked off in parallel right after the
        // optimistic UI - by now routing/model-load usually paid for it.
        const userMemory = await memoryBlockPromise;
        const systemPrompt = buildSystemPrompt(
          selectedAi,
          disposition,
          turnMode,
          userMemory,
          needsVision
        );

        // Generous ceiling — the prompt shapes the actual length; this just prevents
        // truncation. Report mode gets extra headroom because the <think> pass + a
        // comprehensive report can crowd the base ceiling. Capable/online models use
        // it; local models are capped to their context window by the server anyway.
        const dispositionOption = responseLengthOptions.find(
          (opt) => opt.id === disposition
        );
        const maxTokens =
          turnMode === "report" ? 16384 : (dispositionOption?.maxTokens || 8192);

        let fullResponse = "";
        let updateCount = 0;
        let lastFlushAt = 0;
        let firstChunkTime: number | null = null;
        let tokenUsage: {
          prompt_tokens: number;
          completion_tokens: number;
          total_tokens: number;
          provider_fingerprint?: string | null;
          tokens_per_second?: number | null;
        } | null = null;
        let sources: { url: string; title: string }[] | null = null;

        // Thinking belongs to report mode (forces native reasoning via enable_thinking
        // → Thoughts button + transcript + live box). Dispositions are purely length.
        // "Always report" is served by an AI's defaultMode = report. Spontaneous
        // <think> on any mode is still parsed by extractThinkingFromContent.
        const captureThinking = turnMode === "report";

        // Reasoning effort: frontier reasoning models think for seconds at
        // their DEFAULT effort before the first token - even on small talk.
        // A plain conversational turn asks for low effort; anything the
        // classifier upgraded (report/code), an explicit sparkle, an image
        // turn, or a turn the classifier never judged keeps the provider
        // default. The proxy forwards the dial only to providers that
        // support it.
        const classifierJudged = !actionMode && smartMode;
        const reasoningEffort =
          turnMode === "chat" && classifierJudged && !needsVision
            ? ("low" as const)
            : undefined;

        // Write the user's side now (see recordCtx above). Not awaited: the
        // request below goes out immediately; the assistant record awaits
        // this promise later.
        recordCtx.turnMode = turnMode;
        preRecord = (async () => {
          const recHolochainId = recordCtx.holochainId;
          if (!recHolochainId) return null;
          try {
            if (!state.conversationHash) {
              // Use the first user message as the conversation title
              const title = userInput.length > 80
                ? userInput.substring(0, 80) + "..."
                : userInput;
              const hash = await startConversation(
                recHolochainId,
                selectedAi.label,
                recordCtx.modelName,
                title,
              );
              console.log("[Holochain] Conversation started:", hash);
              if (!hash) {
                console.warn("[Holochain] Failed to start conversation");
                return null;
              }
              state.conversationHash = hash;
            }
            const appVersion = await import("@tauri-apps/api/app")
              .then((m) => m.getVersion())
              .catch(() => "unknown");
            // Record user message - with the attached context the model
            // actually received (hash always; full content under the cap).
            const userAttachment = fileContext
              ? await buildAttachmentInfo(fileContext)
              : undefined;
            const imageInfos =
              images.length > 0
                ? await Promise.all(
                    images.map((url, i) => buildImageAttachmentInfo(url, i)),
                  )
                : undefined;
            const userMsgHash = await recordMessage(
              recHolochainId,
              state.conversationHash!,
              "user",
              userInput,
              state.messageSequence,
              "user",
              undefined,
              undefined,
              {
                mode: turnMode,
                attachments: userAttachment,
                images: imageInfos,
              },
            );
            state.messageSequence++;
            console.log("[Holochain] User message recorded");
            return { userMsgHash, userAttachment, imageInfos, appVersion };
          } catch (e) {
            console.warn("[Holochain] Transcript recording failed (user turn):", e);
            return null;
          }
        })();

        for await (const chunk of llamaServerApi.chatCompletion(
          chatHistory,
          systemPrompt,
          controller.signal,
          maxTokens,
          captureThinking,
          preferredModel || undefined,
          undefined,
          reasoningEffort,
          state.cacheKey
        )) {
          if (chunk.type === "status") {
            props.isModelLoading.value = true;
            state.messages = state.messages.map((m) =>
              m.id === assistantId
                ? { ...m, statusText: chunk.content, isLoading: true, content: "" }
                : m
            );
            continue;
          }

          if (chunk.type === "search_status") {
            // Live web-search progress - status line only; deliberately does
            // NOT touch isModelLoading (nothing is loading).
            state.messages = state.messages.map((m) =>
              m.id === assistantId ? { ...m, statusText: chunk.content } : m,
            );
            continue;
          }

          if (chunk.type === "usage") {
            tokenUsage = chunk.data;
            continue;
          }

          if (chunk.type === "sources") {
            sources = chunk.data;
            continue;
          }

          // chunk.type === "text"
          if (updateCount === 0) {
            firstChunkTime = Date.now();
            props.isModelLoading.value = false;
            state.messages = state.messages.map((m) =>
              m.id === assistantId
                ? { ...m, statusText: undefined }
                : m
            );
          }

          fullResponse += chunk.content;
          updateCount++;

          // Throttle UI flushes to ~20/s. Fast models emit 40-70 chunks/s and
          // each flush re-parses the whole accumulated reply and replaces the
          // messages array - per-chunk that's quadratic work the screen can't
          // even display. The final parse after the loop always runs, so a
          // trailing partial between flushes is never lost.
          const now = Date.now();
          if (now - lastFlushAt < 50) continue;
          lastFlushAt = now;

          // Skip the multi-regex parse entirely when no marker is present
          // (the common case - most chat replies have no thinking/wrappers).
          const { thinking, contentWithoutThinking } = PARSE_MARKERS_RE.test(fullResponse)
            ? extractThinkingFromContent(fullResponse)
            : { thinking: "", contentWithoutThinking: fullResponse };

          // Always save thinking — models like Qwen 3.5 produce <think> even in chat mode
          state.messages = state.messages.map((m) =>
            m.id === assistantId
              ? {
                  ...m,
                  content: contentWithoutThinking,
                  thinking: thinking || undefined,
                  statusText: undefined,
                  isLoading: true,
                }
              : m
          );
        }

        // Calculate tokens per second
        const streamEndTime = Date.now();
        let tokensPerSecond: number | undefined;
        if (tokenUsage && tokenUsage.tokens_per_second && tokenUsage.tokens_per_second > 0) {
          // The local server's own measurement - exact, and right for short
          // replies where the app's first-to-last-chunk clock is not.
          tokensPerSecond = Math.round(tokenUsage.tokens_per_second * 10) / 10;
        } else if (tokenUsage && firstChunkTime) {
          // Wall clock from the first streamed chunk: content arrives in whole
          // lines, so a reply that lands as one chunk has no measurable span -
          // only trust spans long enough to mean something.
          const durationSec = (streamEndTime - firstChunkTime) / 1000;
          if (durationSec >= 0.5 && tokenUsage.completion_tokens >= 16) {
            tokensPerSecond = Math.round((tokenUsage.completion_tokens / durationSec) * 10) / 10;
            // This machine's measured speed for an offline model file - what
            // the Models page shows as "~N tok/s measured" (never a bench number).
          }
        }
        // The machine's measured speed per model is recorded engine-side
        // (model-stats.json) from the same server timing; the Models page
        // and the router read that one source.

        // Finalize
        const { thinking: finalThinking, contentWithoutThinking: finalContent } =
          extractThinkingFromContent(fullResponse);

        // If the model only produced thinking with no visible content, show a fallback
        const displayContent = finalContent || (finalThinking ? "(The model's response was all internal reasoning. Try asking again or use a different model.)" : "");

        state.messages = state.messages.map((m) =>
          m.id === assistantId
            ? {
                ...m,
                content: displayContent,
                thinking: finalThinking || undefined,
                isLoading: false,
                tokens: tokenUsage
                  ? {
                      prompt_tokens: tokenUsage.prompt_tokens,
                      completion_tokens: tokenUsage.completion_tokens,
                      total_tokens: tokenUsage.total_tokens,
                      tokens_per_second: tokensPerSecond,
                    }
                  : undefined,
                sources: sources || undefined,
              }
            : m
        );

        // Memory: learn durable user facts from this turn. Fire this BEFORE and
        // INDEPENDENTLY of the Holochain transcript write below — memory must
        // work even when transcripts aren't recording (agent not yet
        // provisioned, conductor still warming), and starting it early maximizes
        // the chance it finishes before the user closes the app.
        //
        // Extraction ALWAYS runs on a LOCAL model. For a local chat that's the
        // chat's own model; for an ONLINE chat we fall back to whatever local
        // model is loaded (the turn already went to the cloud for the reply, so
        // local extraction adds no new exposure). Skip only if no local model
        // is available.
        // Provenance: extraction needs the user message's signed action hash to
        // link each learned fact back to its conversation. But the hash comes
        // from the Holochain write below, which runs in parallel and may be slow
        // or fail. So hand extraction a PROMISE for the hash instead of blocking
        // on it — extraction spends seconds generating before it saves, by which
        // time the write has resolved this (or settled it to null if Holochain
        // is down, degrading gracefully to an unlinked fact).
        let settleUserMsgHash: (h: string | null) => void = () => {};
        const userMsgHashPromise = new Promise<string | null>((res) => {
          settleUserMsgHash = res;
        });

        const localCurrent = props.currentModel.value;
        // Extraction worker (see ON_DEVICE_COMPONENTS.md): the on-device utility
        // model is preferred inside extractAndStoreFacts; this is the FALLBACK
        // for when it isn't installed — a warm offline model, or the online model
        // for an online-capable AI (the turn already went online, so no new
        // exposure). Offline / offline-auto AIs never get an online fallback —
        // the hard privacy wall.
        const warmOffline =
          !isOnlineModel && !isExternalModel && preferredModel
            ? preferredModel
            : localCurrent &&
                !localCurrent.startsWith("online:") &&
                !localCurrent.startsWith("external:")
              ? localCurrent
              : null;
        // Remote turns (proxy or the user's own external server) may extract
        // via the same remote model — the content already went there.
        let extractionFallback =
          warmOffline ?? (isOnlineModel || isExternalModel ? preferredModel : null);
        // gpt-oss speaks its own channel format and fights the extraction
        // grammar (junk output, then a cancelled task): never use it as the
        // extraction fallback. With the on-device utility model installed
        // the fallback is not consulted, so extraction still runs.
        if (
          extractionFallback &&
          /gpt-oss/i.test(extractionFallback) &&
          !(await isUtilityModelReady())
        ) {
          extractionFallback = null;
        }
        // Pre-gate: only spend a model call when the message could state a
        // personal fact (skips pure questions/requests/lookups).
        if (extractionFallback && looksLikeUserFact(userInput)) {
          console.log(`[Memory] Extracting facts from turn (fallback: ${extractionFallback})…`);
          void extractAndStoreFacts({
            userText: userInput,
            model: extractionFallback,
            aiId: selectedAi.id,
            sourceActionHash: userMsgHashPromise,
          });
        } else {
          console.log(
            `[Memory] Extraction skipped — ${!extractionFallback ? "no model" : "no personal fact in message"}`,
          );
        }

        // Record transcript to Holochain (fire-and-forget)
        const holochainId = selectedAi.aiConfig?.agentPubKey;
        const modelName = preferredModel || currentModel || "unknown";

        // Stamp the routing receipt on the assistant message (the hover strip
        // renders it once the reply finishes).
        state.messages = state.messages.map((m) =>
          m.id === assistantId
            ? { ...m, servedBy: modelName, routingReason: routedReason, routingTask: routedTask }
            : m,
        );

        if (!holochainId) {
          console.warn("[Holochain] No agent pub key, skipping transcript");
        }

        (async () => {
          if (!holochainId) {
            settleUserMsgHash(null); // no transcript → fact stays unlinked
            return;
          }
          try {
            // The user's side was written at send time - pick up its result.
            const pre = await preRecord;
            if (!pre) {
              settleUserMsgHash(null);
              return;
            }
            const { userAttachment, imageInfos, appVersion } = pre;
            // Hand the signed hash to the in-flight extraction for provenance.
            settleUserMsgHash(pre.userMsgHash);

            // Source-grounding: anchor the answer's factual claims to the attached
            // document (verbatim quote + char span) and link any attached image,
            // recorded on the ASSISTANT message's provenance — the verifiable chain
            // claim → quote+offset → doc SHA-256 → DHT. Local + best-effort.
            const grounded: GroundedSource[] = [];
            for (const img of imageInfos ?? []) {
              grounded.push({
                kind: "image",
                doc_sha256: img.sha256,
                doc_name: img.filename,
              });
            }
            if (userAttachment && displayContent.trim()) {
              const docName = fileContext?.match(/---\s*(.+?)\s*---/)?.[1];
              const autoGround =
                localStorage.getItem("groundDocumentsAuto") === "true";
              if (autoGround && warmOffline) {
                // Flag "verifying sources…" — the doc-grounding pass is a full
                // local inference and can take many seconds on modest hardware.
                state.messages = state.messages.map((m) =>
                  m.id === assistantId ? { ...m, groundingPending: true } : m,
                );
                grounded.push(
                  ...(await groundDocument({
                    documentText: fileContext!,
                    answerText: displayContent,
                    docSha256: userAttachment.sha256,
                    docName,
                    model: warmOffline,
                  })),
                );
              } else {
                // Auto-grounding off → stash the context so the on-demand "Verify
                // sources" button can run the pass for this answer later.
                state.messages = state.messages.map((m) =>
                  m.id === assistantId
                    ? {
                        ...m,
                        groundingSource: {
                          documentText: fileContext!,
                          docSha256: userAttachment.sha256,
                          docName,
                        },
                      }
                    : m,
                );
              }
            }
            // Surface grounding on the live message (its Sources panel) and clear
            // the verifying flag.
            state.messages = state.messages.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    ...(grounded.length > 0 ? { grounded } : {}),
                    groundingPending: false,
                  }
                : m,
            );

            // Record assistant message — with full provenance: sources, the
            // exact system prompt it ran under, mode, and the runtime/build.
            const assistantMsgHash = await recordMessage(
              holochainId,
              state.conversationHash!,
              "assistant",
              displayContent,
              state.messageSequence,
              modelName,
              finalThinking || undefined,
              tokenUsage ? {
                prompt_tokens: tokenUsage.prompt_tokens,
                completion_tokens: tokenUsage.completion_tokens,
                total_tokens: tokenUsage.total_tokens,
                tokens_per_second: tokensPerSecond,
              } : undefined,
              {
                sources: sources || undefined,
                system_prompt: systemPrompt,
                mode: turnMode,
                grounded: grounded.length > 0 ? grounded : undefined,
                runtime: {
                  // "online" in the receipt means the reply was generated
                  // outside this app — the proxy OR the user's own external
                  // server both qualify.
                  online: isOnlineModel || isExternalModel,
                  app_version: appVersion,
                  max_tokens: maxTokens,
                },
                routing_reason: routedReason,
                routing_task: routedTask,
                // The provider's claim about the backend that answered - the
                // online sibling of the offline model-file hash.
                provider_fingerprint: tokenUsage?.provider_fingerprint || undefined,
              },
            );
            state.messageSequence++;
            // Remember the assistant entry's hash so the on-demand "Verify
            // sources" button can persist its grounding linked to this message.
            if (assistantMsgHash) {
              state.messages = state.messages.map((m) =>
                m.id === assistantId
                  ? { ...m, transcriptHash: assistantMsgHash }
                  : m,
              );
            }
            console.log(
              `[Holochain] Assistant message recorded (${grounded.length} grounded source${grounded.length === 1 ? "" : "s"})`,
            );

            // Index this turn into the AI's episodic memory so it can recall
            // this exchange in future conversations (fire-and-forget; no-ops
            // without the embedding model). Uses the conversation hash so recall
            // can exclude the current conversation.
            // Skip turns answered online: those are freshness/ephemeral queries
            // ("is it raining", "latest news"), so memorizing them just pollutes
            // recall. The append-only transcript above still records them for audit.
            if (!isOnlineModel) {
              void indexTurn({
                aiId: selectedAi.id,
                conversationHash: state.conversationHash!,
                userText: userInput,
                assistantText: displayContent,
              });
            }
          } catch (e) {
            console.warn("[Holochain] Transcript recording failed:", e);
            settleUserMsgHash(null); // write failed (e.g. mid user-message) → unlinked
          }
        })();
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") {
          // Stopped by the user: keep what was said (marked stopped) and
          // record it as far as it got, so the record matches what you saw.
          // Stopped before the first word = nothing to keep.
          const partialMsg = state.messages.find((m) => m.id === assistantId);
          const { thinking: partialThinking, contentWithoutThinking: partialText } =
            extractThinkingFromContent(partialMsg?.content ?? "");
          if (partialText.trim() && !partialMsg?.error) {
            state.messages = state.messages.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    content: partialText,
                    thinking: partialThinking || m.thinking,
                    isLoading: false,
                    statusText: undefined,
                    stopped: true,
                  }
                : m,
            );
            const stoppedPre = preRecord;
            void (async () => {
              const pre = stoppedPre ? await stoppedPre : null;
              if (!pre || !recordCtx.holochainId || !state.conversationHash) return;
              try {
                await recordMessage(
                  recordCtx.holochainId,
                  state.conversationHash,
                  "assistant",
                  partialText,
                  state.messageSequence,
                  recordCtx.modelName,
                  partialThinking || undefined,
                  undefined,
                  {
                    mode: recordCtx.turnMode,
                    stopped: true,
                    runtime: {
                      online:
                        recordCtx.modelName.startsWith("online:") ||
                        recordCtx.modelName.startsWith("external:"),
                      app_version: pre.appVersion,
                    },
                  },
                );
                state.messageSequence++;
                console.log("[Holochain] Stopped reply recorded as far as it got");
              } catch (e) {
                console.warn("[Holochain] Stopped reply not recorded:", e);
              }
            })();
          } else {
            state.messages = state.messages.filter((m) => m.id !== assistantId);
          }
        } else {
          const errorMsg =
            err instanceof Error ? err.message : "Failed to get response";
          console.error("[useChat] Error:", errorMsg);

          const isNoModelError =
            errorMsg.includes("Model file not found") ||
            errorMsg.includes("No model loaded");

          const isModelLoadError =
            errorMsg.includes("Model loading") ||
            errorMsg.includes("Model is taking too long") ||
            errorMsg.includes("Server not ready") ||
            errorMsg.includes("Failed to load model") ||
            errorMsg.includes("failed to load model");

          if (isNoModelError) {
            state.error = "NO_MODELS_AVAILABLE";
            state.messages = state.messages.filter(
              (m) => m.id !== assistantId
            );
          } else if (isModelLoadError) {
            state.error = `This model couldn't be loaded — it's likely too large for your computer's memory. Try a smaller model from the Offline Models page.`;
            state.messages = state.messages.filter(
              (m) => m.id !== assistantId
            );
          } else if (
            errorMsg.includes("Failed to reach the online-model service") ||
            errorMsg.includes("error sending request")
          ) {
            // Online-model proxy unreachable → a friendly, actionable banner
            // (chat route) instead of the raw reqwest/URL error. For an offline
            // online-only AI it offers a one-tap switch to auto online/offline.
            // Stash the turn so the banner can re-send it (no retyping).
            const offline =
              typeof navigator !== "undefined" && navigator.onLine === false;
            const isOnlineOnly = !!preferredModel?.startsWith("online:");
            state.messages = state.messages.filter((m) => m.id !== assistantId);
            state.pendingTurn = {
              userInput,
              chatAction,
              images: images ?? [],
              fileContext,
              visionModel: "",
            };
            state.error = JSON.stringify({
              code: "online_unreachable",
              offline,
              canSwitchAuto: offline && isOnlineOnly,
              aiId: selectedAi.id,
              aiLabel: selectedAi.label,
            });
          } else if (extractOnlineError(errorMsg)) {
            // Billing/auth from the online proxy ({"code":"allowance_exceeded"}
            // and friends) → the standard action cards, never raw JSON in the
            // bubble.
            state.messages = state.messages.filter((m) => m.id !== assistantId);
            state.error = JSON.stringify(extractOnlineError(errorMsg));
          } else {
            state.messages = state.messages.map((m) =>
              m.id === assistantId
                ? {
                    ...m,
                    content: `Error: ${errorMsg}`,
                    error: errorMsg,
                    isLoading: false,
                  }
                : m
            );
          }
        }
      } finally {
        state.isLoading = false;
        props.isModelLoading.value = false;
        abortControllerRef.value = null;
      }
    }
  );

  const stopGeneration = $(async () => {
    if (abortControllerRef.value) {
      abortControllerRef.value.abort();
    }
    // Also cancel the Rust-side streaming request
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("cancel_chat_completion");
    } catch (e) {
      console.error("[useChat] Failed to cancel Rust stream:", e);
    }
  });

  const resetChat = $(() => {
    if (abortControllerRef.value) {
      abortControllerRef.value.abort();
      abortControllerRef.value = null;
    }
    state.messages = [];
    state.error = null;
    state.isLoading = false;
    state.conversationHash = null;
    state.cacheKey = newCacheKey();
    state.messageSequence = 0;
  });

  const retry = $(async (messageId: string, target?: "online" | "device") => {
    const messageIndex = state.messages.findIndex((m) => m.id === messageId);
    if (messageIndex === -1 || messageIndex === 0) return;

    const userMessage = state.messages[messageIndex - 1];
    if (userMessage.role !== "user") return;

    // "Try online" / "Redo on device": resolve the destination through the
    // router so the user's per-slot online picks and offline lean apply, then
    // resend with the concrete model. Falls back to a plain retry if routing
    // can't deliver the asked-for side.
    let override: string | undefined;
    if (target) {
      const task = state.messages[messageIndex]?.routingTask || "general";
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const r = await invoke<{ model: string; reason: string }>("route_model", {
          mode: target === "online" ? "online-offline" : "offline",
          query: userMessage.content,
          task,
          // "hard" routes straight to the stronger-model slot; eagerness
          // privacy keeps freshness from hijacking an explicit request.
          difficulty: target === "online" ? "hard" : "easy",
          eagerness: "privacy",
          lean: localStorage.getItem("routingOfflineLean") || "balanced",
          onlineFresh: localStorage.getItem("routingOnlineFresh") || undefined,
          onlineHardCode: localStorage.getItem("routingOnlineHardCode") || undefined,
          onlineHardGeneral: localStorage.getItem("routingOnlineHardGeneral") || undefined,
          turnTokens: Math.ceil(
            (state.messages.slice(0, messageIndex).reduce((n, m) => n + (m.content?.length ?? 0), 0) +
              userMessage.content.length +
              4000) / 4,
          ) + (userMessage.images?.length ?? 0) * 1000,
        });
        const wentOnline = r.model.startsWith("online:");
        if (target === "online" ? wentOnline : !wentOnline) override = r.model;
      } catch (e) {
        console.warn("[Router] retry-with-target resolve failed:", e);
      }
    }

    state.messages = state.messages.slice(0, messageIndex);
    await sendMessage(userMessage.content, null, undefined, userMessage.images || [], override);
  });

  // On-demand source-grounding for a past document answer (the "Verify sources"
  // button shown when auto-grounding is off). Uses the currently-loaded local
  // model; display-only (the append-only transcript entry already exists).
  const groundMessage$ = $(async (messageId: string) => {
    const msg = state.messages.find((m) => m.id === messageId);
    if (!msg?.groundingSource || msg.groundingPending) return;
    const current = props.currentModel.value;
    const model =
      current && !current.startsWith("online:") && !current.startsWith("external:")
        ? current
        : null;
    if (!model) {
      state.messages = state.messages.map((m) =>
        m.id === messageId
          ? { ...m, statusText: "Load an offline model to verify sources." }
          : m,
      );
      return;
    }
    state.messages = state.messages.map((m) =>
      m.id === messageId ? { ...m, groundingPending: true, statusText: undefined } : m,
    );
    const grounded = await groundDocument({
      documentText: msg.groundingSource.documentText,
      answerText: msg.content,
      docSha256: msg.groundingSource.docSha256,
      docName: msg.groundingSource.docName,
      model,
    });
    state.messages = state.messages.map((m) =>
      m.id === messageId
        ? {
            ...m,
            ...(grounded.length > 0 ? { grounded } : {}),
            groundingPending: false,
          }
        : m,
    );
    // Persist it as an annotation linked to this message, so it survives a
    // reload (auto-grounding rides with the message; on-demand needs this).
    const holochainId = props.selectedAi.value.aiConfig?.agentPubKey;
    if (grounded.length > 0 && holochainId && state.conversationHash && msg.transcriptHash) {
      await recordGroundingAnnotation(
        holochainId,
        state.conversationHash,
        msg.transcriptHash,
        grounded,
      );
    }
  });

  return {
    state,
    sendMessage,
    stopGeneration,
    resetChat,
    retry,
    groundMessage$,
  };
}
