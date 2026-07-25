/**
 * Chat Page (Qwik City route) - Main conversation interface
 * Migrated from React ChatPage.tsx
 */

import {
  component$,
  useSignal,
  useContext,
  useVisibleTask$,
  $,
} from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";

import { ThemeContext } from "../layout";
import { useAiData, useAiDataActions } from "../../contexts/AiDataContext";
import { useChat } from "../../hooks/useChat";
import { useAgentSession, readRecentFolders, resolveBinaryPath } from "../../hooks/useAgentSession";
import { ConversationsDrawer } from "../../components/ConversationsDrawer";
import {
  listAllConversations,
  loadConversationMessages,
  getConversationFolder,
  rememberLastConversation,
  readLastConversation,
  sanitizeTitle,
  setConversationTitleOverride,
  getConversationTitleOverride,
  type ConversationListItem,
  type LastConversationPointer,
} from "../../utils/conversationResume";
import { waitForHolochainReady } from "../../utils/holochainTranscripts";
import ConfirmModal from "../../components/ConfirmModal";
import { drainPendingExtractions, prewarmExtractionModel } from "../../utils/memoryExtraction";
import GpuSafeModeNotice from "../../components/GpuSafeModeNotice";
import { VisionDownloadCard } from "../../components/VisionDownloadCard";
import { OcrAttachStatus } from "../../components/OcrAttachStatus";
import { useVisionDownload } from "../../contexts/VisionDownloadContext";

import ChatContainer from "../../components/ChatContainer";
import CodePanel from "../../components/CodePanel";
import AppHeader from "../../components/AppHeader";
import WelcomeModal from "../../components/WelcomeModal";

import type { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from "../../types";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import veeboThumb from "../../assets/veebo-thumb.jpg";

/** Module-level flag — survives Qwik dev-mode re-mounts */
let needsWelcomeModal = false;

/** SystemInfo shape returned by Tauri's get_system_info command */
interface SystemInfo {
  total_memory_gb: number;
  used_memory_gb: number;
  cpu_count: number;
  os_name: string;
  os_version: string;
  gpu_name: string | null;
  total_vram_gb: number | null;
}

export default component$(() => {
  const nav = useNavigate();
  const { theme } = useContext(ThemeContext);

  const aiDataState = useAiData();
  const {
    updateAllAisWithFirstModel,
    updateCustomAi,
  } = useAiDataActions();

  // --- Signals (replace useState) ---
  const currentModel = useSignal<string | null>(null);
  const input = useSignal("");
  const selectedAction = useSignal<ChatAction>(null);
  const sidePanelContent = useSignal<{
    messageId: string;
    codeString: string;
    language: string;
  } | null>(null);
  const isSidePanelVisible = useSignal(true);
  const isModelLoading = useSignal(false);
  const modelLoadTime = useSignal<number | null>(null);
  const modelTooBig = useSignal(false);
  const showModelWidget = useSignal(false);
  const showWelcomeModal = useSignal(needsWelcomeModal);
  const systemInfo = useSignal<SystemInfo | null>(null);
  const attachedFiles = useSignal<AttachedFile[]>([]);
  const attachedImages = useSignal<AttachedImage[]>([]);
  // A scanned PDF the user attached while OCR isn't installed — drives the offer.
  const ocrNeeded = useSignal<{ filePath: string; filename: string } | null>(null);
  // A scanned PDF currently being OCR'd (installed path) — drives the "reading…"
  // notice, since OCR can take a while and scales with page count.
  const ocrProcessing = useSignal<string | null>(null);
  const contextWindowSize = useSignal(8192);

  // Refs (replace useRef)
  const messagesEndRef = useSignal<HTMLDivElement | undefined>();
  const hasCheckedForModels = useSignal(false);

  // Selected AI (placeholder until data loads from store)
  // Uses a non-matching ID so TASK B auto-switches to the real first active AI on hydration
  const selectedAi = useSignal<SelectedAiModel>({
    id: "__placeholder__",
    label: "",
    imageUrl: veeboThumb,
    aiConfig: {
      id: "__placeholder__",
      name: "",
      description: "",
      baseArchetypeId: "veebo",
      model: "",
      systemPrompt: "",
      status: "active",
      lengthDisposition: "conversational",
      defaultMode: "chat",
      useEmojis: false,
    },
  });

  const visionDownload = useVisionDownload();

  // useChat hook — pass signals directly
  const {
    state: chatState,
    sendMessage,
    stopGeneration,
    resetChat,
    retry,
    groundMessage$,
  } = useChat({
    selectedAi,
    currentModel,
    isModelLoading,
    modelLoadTime,
    modelTooBig,
  });

  // Folder (Build agent) session - while a folder is open, every prompt goes
  // through the agent instead of the direct model call (one brain).
  const {
    agentState,
    openFolder$,
    closeFolder$,
    sendPrompt$: sendAgentPrompt$,
    cancelTurn$,
    respondPermission$,
    answerPermissionByReply$,
    acceptOverloadOffer$,
    dismissOverloadOffer$,
  } = useAgentSession({ chatState, selectedAi });
  const showCloseFolderConfirm = useSignal(false);
  // The header workspace slot: exists only when Build is installed.
  const buildInstalled = useSignal(false);
  const recentFolders = useSignal<string[]>([]);


  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => agentState.folderPath);
    recentFolders.value = readRecentFolders();
    if (!buildInstalled.value) {
      invoke<boolean>("path_is_file", { path: resolveBinaryPath() })
        .then(async (present) => {
          if (present) {
            buildInstalled.value = true;
            return;
          }
          // The saved path can go stale (cleared storage, moved data dir) -
          // the installer's own record is the fallback truth.
          try {
            const s = (await invoke("build_install_status")) as {
              installed: boolean;
              path: string | null;
            };
            if (s.installed && s.path) {
              localStorage.setItem("build-binary-path", s.path);
              buildInstalled.value = true;
            }
          } catch {
            buildInstalled.value = false;
          }
        })
        .catch(() => (buildInstalled.value = false));
    }
    // Install completion can land while any page is open - flip the gate.
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen("build-install-done", () => (buildInstalled.value = true)),
    );
    // The bridge owns the workspace: after navigating away and back (the
    // route remounts), pick the open folder back up from its status.
    if (!agentState.folderPath) {
      invoke<{ running: boolean; folder: string | null }>("build_agent_status")
        .then((status) => {
          if (status.folder && !agentState.folderPath) {
            agentState.folderPath = status.folder;
            agentState.status = status.running ? "ready" : "stopped";
          }
        })
        .catch(() => {});
    }
  });

  // --- Build dynamic model options from unified AI list ---
  const dynamicModelOptions = useSignal<SelectedAiModel[]>([]);
  const currentSelectedOptionInListbox = useSignal<SelectedAiModel | undefined>(undefined);

  // Conversations: the temporal lens - every conversation is a place.
  const conversationsOpen = useSignal(false);
  const conversationsLoading = useSignal(false);
  const conversationItems = useSignal<ConversationListItem[]>([]);
  const lastConversation = useSignal<LastConversationPointer | null>(null);
  // A folder the resumed conversation worked in (ask before switching).
  const resumeFolderAsk = useSignal<string | null>(null);

  // Folder guard: opening a workspace with an AI whose model can't drive
  // agent work offers a switch - the folder still opens (workspace is
  // app-wide), and there is NEVER a silent model substitution. Auto-routing
  // AIs (score -1) pass: the router decides per turn.
  const folderGuard = useSignal<{
    path: string;
    currentLabel: string;
    suggestion?: SelectedAiModel;
  } | null>(null);
  // Any attempt to open a project without the Build add-on funnels here -
  // one modal, one download, installs in the background.
  const installAsk = useSignal(false);

  const openFolderGuarded = $(async (path: string) => {
    if (!buildInstalled.value) {
      installAsk.value = true;
      return;
    }
    openFolder$(path);
    try {
      const score = await invoke<number>("agent_capability", {
        model: selectedAi.value.aiConfig?.model || "",
      });
      if (score >= 0 && score < 6) {
        let suggestion: SelectedAiModel | undefined;
        for (const option of dynamicModelOptions.value) {
          if (option.id === selectedAi.value.id) continue;
          const s = await invoke<number>("agent_capability", {
            model: option.aiConfig?.model || "",
          });
          if (s >= 6) {
            suggestion = option;
            break;
          }
        }
        folderGuard.value = {
          path,
          currentLabel: selectedAi.value.label,
          suggestion,
        };
      }
    } catch {
      /* the guard is advisory - never block the folder on it */
    }
  });

  /** Re-enter a conversation: rebuild the messages from its transcript and
   *  keep appending to the same hash. */
  const resumeConversation = $(
    async (target: { hash: string; agentKey: string; aiId?: string }) => {
      if (chatState.isLoading) return;
      conversationsOpen.value = false;
      // Match the AI by agent key first (robust), then by id.
      const ai =
        dynamicModelOptions.value.find(
          (o) => o.aiConfig?.agentPubKey === target.agentKey,
        ) ??
        dynamicModelOptions.value.find((o) => o.id === target.aiId) ??
        selectedAi.value;
      selectedAi.value = ai;
      resetChat();
      // A resume fired right after launch (the Memory-page handoff or the
      // hero Continue line) can outrun the conductor - reads would come
      // back empty and the conversation would look blank or unanswered.
      await waitForHolochainReady();
      const { messages, nextSequence, folderPath } = await loadConversationMessages(
        target.agentKey,
        target.hash,
        ai,
      );
      if (messages.length === 0) return;
      chatState.messages = messages;
      chatState.conversationHash = target.hash;
      chatState.messageSequence = nextSequence;
      const firstUser = messages.find((m) => m.role === "user");
      rememberLastConversation({
        hash: target.hash,
        agentKey: target.agentKey,
        aiId: ai.id,
        title: firstUser?.content.slice(0, 80) ?? "Conversation",
      });
      // Worked in a folder? Ask before switching the workspace - never
      // silently swap it. The transcript's own record wins; the client map
      // covers conversations from before the transcript carried it.
      const folder = folderPath ?? getConversationFolder(target.hash);
      if (folder && folder !== agentState.folderPath) {
        resumeFolderAsk.value = folder;
      }
    },
  );

  const openConversations = $(async () => {
    conversationsOpen.value = true;
    conversationsLoading.value = true;
    try {
      // Right after app launch the conductor is still starting and reads
      // come back empty - hold the spinner until it's actually up, so the
      // drawer never shows "nothing yet" about records that exist.
      await waitForHolochainReady();
      conversationItems.value = await listAllConversations(
        dynamicModelOptions.value,
      );
    } finally {
      conversationsLoading.value = false;
    }
  });

  // Last-conversation pointer: written whenever a chat-path conversation
  // starts (the agent path writes its own), read for the hero line. Also
  // honor a resume handoff from the Memory page (sessionStorage - query
  // params are unreliable in the packaged app).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const hash = track(() => chatState.conversationHash);
    lastConversation.value = readLastConversation();
    if (hash) {
      const firstUser = chatState.messages.find((m) => m.role === "user");
      const agentKey = selectedAi.value.aiConfig?.agentPubKey;
      if (agentKey) {
        rememberLastConversation({
          hash,
          agentKey,
          aiId: selectedAi.value.id,
          title: firstUser?.content.slice(0, 80) ?? "Conversation",
        });
        lastConversation.value = readLastConversation();
      }
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    try {
      if (sessionStorage.getItem("open-conversations")) {
        sessionStorage.removeItem("open-conversations");
        setTimeout(() => openConversations(), 300);
      }
      const pendingFolder = sessionStorage.getItem("pending-open-folder");
      if (pendingFolder) {
        sessionStorage.removeItem("pending-open-folder");
        setTimeout(() => openFolderGuarded(pendingFolder), 300);
      }
      const raw = sessionStorage.getItem("resume-conversation");
      if (raw) {
        sessionStorage.removeItem("resume-conversation");
        const target = JSON.parse(raw);
        if (target?.hash && target?.agentKey) {
          // Model options hydrate async - give them a beat.
          setTimeout(() => resumeConversation(target), 400);
        }
      }
    } catch {
      /* no handoff */
    }
  });

  // Auto-resume a message that was waiting on a vision-model download, once the
  // app-level manager reports it finished for this AI (fires on completion or when
  // returning to this chat). consumeReady$ no-ops if it's for a different AI.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const status = track(() => visionDownload.state.active?.status);
    track(() => selectedAi.value.id);
    if (status !== "done") return;
    const ready = await visionDownload.consumeReady$(selectedAi.value.id);
    if (ready?.pendingTurn) {
      await sendMessage(
        ready.pendingTurn.userInput,
        ready.pendingTurn.chatAction,
        ready.pendingTurn.fileContext,
        ready.pendingTurn.images,
        ready.visionModel,
      );
    }
  });

  // ==========================================================================
  // TASK A: Mount-once initialization
  // Restores localStorage, sets up event listeners, checks/loads models.
  // ==========================================================================
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    // --- Restore localStorage settings ---
    const savedModel = localStorage.getItem("currentModel");
    if (savedModel) {
      currentModel.value = savedModel;
    }
    const savedWidget = localStorage.getItem("showModelWidget");
    showModelWidget.value = savedWidget === "true";

    // --- Event listeners ---
    const handleSettingsChange = (e: Event) => {
      const customEvent = e as CustomEvent;
      if (customEvent.detail?.showModelWidget !== undefined) {
        showModelWidget.value = customEvent.detail.showModelWidget;
      }
    };

    const checkCurrentModelExists = async () => {
      if (!currentModel.value) return;
      try {
        const models = await invoke<any[]>("list_local_models");
        if (!models || models.length === 0) {
          console.log("[ChatPage] No models found, clearing currentModel");
          currentModel.value = null;
          localStorage.removeItem("currentModel");
        } else {
          const modelExists = models.some(
            (m: any) => m.name === currentModel.value
          );
          if (!modelExists) {
            console.log("[ChatPage] Current model no longer exists, clearing");
            currentModel.value = null;
            localStorage.removeItem("currentModel");
          }
        }
      } catch (error) {
        console.error("[ChatPage] Error checking model existence:", error);
      }
    };

    window.addEventListener("settingsChanged", handleSettingsChange);
    window.addEventListener("focus", checkCurrentModelExists);
    window.addEventListener("modelDeleted", checkCurrentModelExists);

    cleanup(() => {
      window.removeEventListener("settingsChanged", handleSettingsChange);
      window.removeEventListener("focus", checkCurrentModelExists);
      window.removeEventListener("modelDeleted", checkCurrentModelExists);
    });

    // Fetch context window size for file attachment token budget
    try {
      const ctxSize = await invoke<number>("get_context_window_size");
      contextWindowSize.value = ctxSize;
    } catch (e) {
      console.error("[ChatPage] Failed to get context window size:", e);
    }

    // --- Check for models on mount, show welcome modal or auto-load ---
    if (hasCheckedForModels.value) {
      console.log("[ChatPage] Skipping duplicate model check");
      return;
    }
    hasCheckedForModels.value = true;

    console.log("[ChatPage] Checking for offline models...");

    // Wait for AiDataContext to finish initializing
    const maxWait = 50;
    for (let i = 0; i < maxWait; i++) {
      if (aiDataState.isInitialized) break;
      await new Promise((r) => setTimeout(r, 200));
    }

    try {
      const sysInfo = await invoke<SystemInfo>("get_system_info");
      systemInfo.value = sysInfo;

      const models = await invoke<any[]>("list_local_models");

      if (!models || models.length === 0) {
        console.log(
          "[ChatPage] No models found, clearing currentModel and showing welcome modal"
        );
        currentModel.value = null;
        localStorage.removeItem("currentModel");
        needsWelcomeModal = true;
        showWelcomeModal.value = true;
        return;
      }

      // Preload the model of the AI you'll actually land on (the last-used AI), so
      // the load happens while you settle in rather than after your first query.
      // Falls back to the last model / first available when that AI's model isn't a
      // single downloaded offline file (e.g. an Auto or online AI — nothing to
      // preload). This is what makes the preload pay off instead of guessing wrong.
      const lastAiId = localStorage.getItem("lastAiId");
      const activeAis = (Array.isArray(aiDataState.userDefinedAis) ? aiDataState.userDefinedAis : [])
        .filter((ai) => ai.status === "active");
      const lastAi = activeAis.find((ai) => ai.id === lastAiId) || activeAis[0];
      const aiModel = lastAi?.model;
      const isLocalFile =
        !!aiModel &&
        !aiModel.startsWith("auto:") &&
        !aiModel.startsWith("online:") &&
        !aiModel.startsWith("external:");
      const targetModel = isLocalFile && models.some((m) => m.name === aiModel)
        ? aiModel
        : localStorage.getItem("currentModel") || models[0].name;

      // Check if this model is already loaded and ready (e.g. returning from another page)
      // Verify via Rust state that the model is actually loaded, not just that the server is healthy
      // (the server can be "ready" in router mode with 0 models loaded)
      const rustModel = await invoke<string | null>("get_current_model");
      const alreadyReady = currentModel.value === targetModel
        && rustModel === targetModel
        && await invoke<boolean>("is_llama_server_ready");

      if (alreadyReady) {
        console.log("[ChatPage] Model already loaded and ready:", targetModel);
        currentModel.value = targetModel;
        modelTooBig.value = false;
      } else {
        console.log("[ChatPage] Loading model:", targetModel);
        isModelLoading.value = true;
        modelLoadTime.value = Date.now();
        try {
          await invoke("load_model", { filename: targetModel, withVision: false, reason: "page-ensure" });
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          if (msg.includes("MODEL_TOO_LARGE") || msg.includes("MODEL_LOAD_CRASHED")) {
            // Won't fit this GPU — show it red in the header rather than a stuck
            // spinner. Nothing's loading, so skip the readiness poll.
            console.log("[ChatPage] Preload model too large for GPU:", targetModel);
            currentModel.value = targetModel;
            modelTooBig.value = true;
            isModelLoading.value = false;
            return;
          }
          throw e;
        }
        modelTooBig.value = false;
        currentModel.value = targetModel;

        console.log("[ChatPage] Waiting for model to be fully ready...");
        const maxAttempts = 60;
        for (let attempt = 1; attempt <= maxAttempts; attempt++) {
          const isReady = await invoke<boolean>("is_llama_server_ready");
          if (isReady) {
            console.log("[ChatPage] Model is fully loaded and ready!");
            break;
          }
          if (attempt % 5 === 0) {
            console.log(
              `[ChatPage] Still waiting for model... (${attempt}/${maxAttempts})`
            );
          }
          if (attempt >= maxAttempts) {
            throw new Error("Model took too long to load (>2 minutes)");
          }
          await new Promise((resolve) => setTimeout(resolve, 2000));
        }

        console.log("[ChatPage] Initial model load complete");
      }

      // A local model is now ready — re-run any extraction that was cut off by
      // an app close last session (e.g. closing right after sending).
      if (
        currentModel.value &&
        !currentModel.value.startsWith("online:") &&
        !currentModel.value.startsWith("external:")
      ) {
        void drainPendingExtractions(currentModel.value);
      }

      // Pre-warm the on-device utility model (if installed) now that startup +
      // the chat model have settled — loads it + primes the extraction prompt
      // cache so the first fact this session isn't a cold ~22s wait. CPU-only,
      // never touches the chat model (GPU); no-op if the model isn't installed.
      void prewarmExtractionModel();
    } catch (error) {
      console.error("[ChatPage] Failed during initialization:", error);
    } finally {
      isModelLoading.value = false;
    }
  });

  // ==========================================================================
  // TASK B: Reactive AI data — rebuilds options & selectedAi when data changes
  // ==========================================================================
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const ais = track(() => aiDataState.userDefinedAis);
    const thumbUrls = track(() => aiDataState.thumbnailObjectUrls);
    const archetypeThumbs = track(() => aiDataState.archetypeDefaultThumbnailsMap);
    const currentAi = track(() => selectedAi.value);
    const isInitialized = track(() => aiDataState.isInitialized);

    // Don't update options until all data (including thumbnails) is loaded
    if (!isInitialized) return;

    // Build active AI options list
    // Use static imports for default AIs to avoid flicker before custom thumbnails load
    const options = (Array.isArray(ais) ? ais : [])
      .filter((ai) => ai.status === "active")
      .map((ai): SelectedAiModel => {
        const customThumbnail = thumbUrls[ai.id];
        const staticThumb = archetypeThumbs[ai.baseArchetypeId];
        const imageUrl = customThumbnail || staticThumb || "/generic-ai-placeholder.svg";
        return {
          id: ai.id,
          label: ai.name,
          imageUrl,
          aiConfig: ai,
        };
      });

    dynamicModelOptions.value = options;
    currentSelectedOptionInListbox.value = options.find(
      (opt) => opt.id === currentAi.id
    );

    // The placeholder (or a now-removed AI) needs a real selection. Prefer the AI
    // you last used — so you resume where you left off AND the startup preload that
    // targeted its model pays off — falling back to the first active AI.
    const lastAiId = localStorage.getItem("lastAiId");
    const target = ais.find((ai) => ai.id === lastAiId && ai.status === "active")
      || ais.find((ai) => ai.status === "active");
    if (target && !options.some((opt) => opt.id === currentAi.id)) {
      const customThumb = thumbUrls[target.id];
      const archetypeThumb = archetypeThumbs[target.baseArchetypeId];
      console.log("[ChatPage] Selecting AI:", target.name);
      selectedAi.value = {
        id: target.id,
        label: target.name,
        imageUrl: customThumb || archetypeThumb || "/generic-ai-placeholder.svg",
        aiConfig: target,
      };
    }
  });

  // ==========================================================================
  // TASK C: Persist currentModel to localStorage when it changes
  // ==========================================================================
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const model = track(() => currentModel.value);
    if (model) {
      localStorage.setItem("currentModel", model);
    }
  });

  // ==========================================================================
  // Remember the last-used AI, so next launch reopens to it AND the
  // startup preload targets ITS model (not a stale "last model").
  // ==========================================================================
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const id = track(() => selectedAi.value.id);
    if (id && id !== "__placeholder__") {
      localStorage.setItem("lastAiId", id);
    }
  });

  // --- Callbacks (replace useCallback with $()) ---
  const handleSubmit = $(async () => {
    if (!input.value.trim()) return;

    // Typing while a permission card waits IS the answer: decline with
    // instructions - the reply shows in place and goes to the agent next.
    if (agentState.pendingPermissionId) {
      const text = input.value;
      input.value = "";
      selectedAction.value = null;
      await answerPermissionByReply$(text);
      return;
    }

    if (chatState.isLoading) return;

    const finalInput = selectedAction.value
      ? `${selectedAction.value.replace(/\.\.\.$/, "")} ${input.value}`
      : input.value;

    // Build file context string if files are attached (sent to AI, not displayed)
    let fileContext: string | undefined;
    if (attachedFiles.value.length > 0) {
      const fileContextParts = attachedFiles.value.map(f =>
        `--- ${f.filename} ---\n${f.content}`
      );
      fileContext = `[Attached files for context]\n\n${fileContextParts.join('\n\n')}`;
    }

    // Folder open -> the agent session is the one brain for this
    // conversation. Attached-file text rides along; images are a direct-chat
    // feature for now.
    if (agentState.folderPath) {
      const agentInput = fileContext
        ? `${fileContext}\n\n${finalInput}`
        : finalInput;
      sendAgentPrompt$(agentInput);
      input.value = "";
      selectedAction.value = null;
      attachedImages.value = [];
      return;
    }

    const images = attachedImages.value.map((i) => i.dataUrl);
    sendMessage(finalInput, selectedAction.value, fileContext, images);
    input.value = "";
    selectedAction.value = null;
    attachedImages.value = []; // image is per-turn — don't silently re-send it
  });

  // Stop: an agent turn cancels over ACP (the agent still ends the turn);
  // a direct model turn aborts the stream.
  const handleStop = $(async () => {
    if (agentState.folderPath && chatState.isLoading) {
      await cancelTurn$();
    } else {
      await stopGeneration();
    }
  });

  // Chip close: instant when idle, one confirm when the agent is mid-task.
  const handleCloseFolder = $(async () => {
    if (chatState.isLoading) {
      showCloseFolderConfirm.value = true;
    } else {
      await closeFolder$();
    }
  });

  const scrollToBottom = $((behavior: ScrollBehavior = "smooth") => {
    const end = messagesEndRef.value;
    if (!end) return;
    // Only auto-follow to the bottom if the user is already near it. If they've
    // scrolled up to read (e.g. a long report), don't yank them back down.
    const container = end.closest(".overflow-y-auto") as HTMLElement | null;
    if (container) {
      const distanceFromBottom =
        container.scrollHeight - container.scrollTop - container.clientHeight;
      if (distanceFromBottom > 150) return;
    }
    end.scrollIntoView({ behavior, block: "end" });
  });

  const handleNewQuestion = $(() => {
    resetChat();
    input.value = "";
    selectedAction.value = null;
    sidePanelContent.value = null;
    attachedFiles.value = [];
    attachedImages.value = [];
    // The workspace is app-wide: a new conversation starts IN it, with a
    // fresh agent session in the same folder. (Process restart for now -
    // the generation guard makes it safe; session-in-place later.)
    if (agentState.folderPath) openFolder$(agentState.folderPath);
  });

  // Send a suggested command to the user's own terminal - visible,
  // interruptible, theirs. Default: pre-filled on the prompt, Enter to run
  // (the final look stays with the user); the setting flips to immediate.
  // Opens in the workspace folder when one is open.
  const handleOpenTerminal = $(async (command: string) => {
    const immediate = localStorage.getItem("terminal-run-immediately") === "true";
    try {
      if (!immediate) {
        // Belt and braces for the press-Enter flow (and cmd on Windows,
        // which cannot pre-fill): the command is also on the clipboard.
        await navigator.clipboard.writeText(command).catch(() => {});
      }
      await invoke("open_in_terminal", {
        command,
        cwd: agentState.folderPath ?? null,
        immediate,
      });
    } catch (err) {
      console.error("[ChatPage] Could not open a terminal:", err);
    }
  });

  const handleBrowseFolder = $(async () => {
    if (!buildInstalled.value) {
      installAsk.value = true;
      return;
    }
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({ directory: true, title: "Choose the project's folder" });
      if (typeof selected === "string" && selected) openFolderGuarded(selected);
    } catch (err) {
      console.error("[ChatPage] Folder picker error:", err);
    }
  });

  const handleAttachFiles = $(async (paths: string[]) => {
    // WebP is excluded: llama.cpp (stb_image) can't decode it — the model would
    // silently receive no image. One image at a time: llama.cpp's multimodal path
    // crashes on more than one image per request, so a new image replaces the last.
    const imageExts = ["png", "jpg", "jpeg", "gif", "bmp"];
    for (const filePath of paths) {
      // Dropping a FOLDER opens it (like dropping a folder on an editor);
      // dropping a file stays an attachment.
      try {
        if (await invoke<boolean>("path_is_dir", { path: filePath })) {
          openFolderGuarded(filePath);
          continue;
        }
      } catch {
        // Classifier unavailable - treat as a file.
      }

      const ext = filePath.split(".").pop()?.toLowerCase() ?? "";

      // Images take the vision path: read as a base64 data URL (not text).
      if (imageExts.includes(ext)) {
        try {
          const img = await invoke<{ filename: string; data_url: string; size_bytes: number }>(
            "read_image_for_context",
            { filePath },
          );
          attachedImages.value = [
            { filename: img.filename, dataUrl: img.data_url, sizeBytes: img.size_bytes },
          ];
        } catch (err) {
          console.error(`[ChatPage] Failed to read image ${filePath}:`, err);
        }
        continue;
      }

      // Skip if already attached
      if (attachedFiles.value.some(f => f.path === filePath)) continue;
      try {
        const result = await invoke<{
          filename: string;
          path: string;
          size_bytes: number;
          content: string;
          truncated: boolean;
          estimated_tokens: number;
        }>("read_file_for_context", { filePath });

        attachedFiles.value = [...attachedFiles.value, {
          filename: result.filename,
          path: result.path,
          sizeBytes: result.size_bytes,
          content: result.content,
          truncated: result.truncated,
          estimatedTokens: result.estimated_tokens,
        }];
      } catch (err) {
        // A PDF that yields no text is almost certainly scanned (image-based) →
        // OCR it if installed, otherwise offer the optional download.
        if (filePath.toLowerCase().endsWith(".pdf")) {
          const filename = filePath.split(/[\\/]/).pop() || "document.pdf";
          try {
            if (await invoke<boolean>("is_ocr_ready")) {
              ocrProcessing.value = filename;
              let text = "";
              try {
                text = await invoke<string>("ocr_scanned_pdf", { filePath });
              } finally {
                ocrProcessing.value = null;
              }
              if (text && text.trim()) {
                attachedFiles.value = [
                  ...attachedFiles.value,
                  {
                    filename,
                    path: filePath,
                    sizeBytes: new TextEncoder().encode(text).length,
                    content: text,
                    truncated: false,
                    estimatedTokens: Math.ceil(text.length / 4),
                  },
                ];
                continue;
              }
            } else {
              ocrNeeded.value = { filePath, filename };
              continue;
            }
          } catch (ocrErr) {
            console.error(`[ChatPage] OCR failed for ${filePath}:`, ocrErr);
          }
        }
        console.error(`[ChatPage] Failed to read file ${filePath}:`, err);
      }
    }
  });

  const handleDownloadModel = $(() => {
    nav("/setup");
  });

  const handleUpgradeClick = $(() => {
    const websiteUrl =
      (import.meta as any).env?.VITE_WEBSITE_URL || "https://yourownai.net";
    import("@tauri-apps/plugin-opener").then((m) => m.openUrl(`${websiteUrl}/pricing`));
  });

  const handleModelDownloaded = $(async (filename: string) => {
    console.log("[ChatPage] First model downloaded:", filename);

    try {
      await updateAllAisWithFirstModel(filename);
      console.log("[ChatPage] Auto-assigned model to all AIs");

      await new Promise((resolve) => setTimeout(resolve, 100));

      // Update selectedAi to reflect the new mode (Auto - Offline Only; the
      // just-downloaded model is what Auto resolves to right now).
      const prev = selectedAi.value;
      selectedAi.value = {
        ...prev,
        aiConfig: { ...prev.aiConfig, model: "auto:offline" },
      };

      currentModel.value = filename;
      isModelLoading.value = true;
      modelLoadTime.value = Date.now();

      // Poll until model is ready
      console.log("[ChatPage] Waiting for model to be fully ready...");
      const maxAttempts = 60;
      for (let attempt = 1; attempt <= maxAttempts; attempt++) {
        const isReady = await invoke<boolean>("is_llama_server_ready");
        if (isReady) {
          console.log("[ChatPage] Model is fully loaded and ready!");
          break;
        }
        if (attempt >= maxAttempts) {
          console.error("[ChatPage] Model took too long to load");
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 2000));
      }

      isModelLoading.value = false;
    } catch (error) {
      console.error("[ChatPage] Error during model setup:", error);
      isModelLoading.value = false;
    }
  });

  // --- Derived state ---
  const isDesktop = true;
  const showSidePanel =
    sidePanelContent.value && isDesktop && isSidePanelVisible.value;

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      {/* Header */}
      <div class="relative z-10">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleDownloadModel}
          currentModel={currentModel.value}
          isModelLoading={isModelLoading.value}
          modelTooBig={modelTooBig.value}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
          folderPath={agentState.folderPath}
          folderStatus={
            agentState.status === "idle" ? undefined : agentState.status
          }
          onCloseFolder$={handleCloseFolder}
          buildInstalled={buildInstalled.value}
          recentFolders={recentFolders.value}
          onOpenFolder$={openFolderGuarded}
          onBrowseFolder$={handleBrowseFolder}
          onOpenConversations$={openConversations}
        />
      </div>

      <GpuSafeModeNotice />

      {/* Main Content */}
      <div class="relative flex-1 flex flex-col min-h-0">
        <main class="flex-1 flex flex-row bg-[var(--bg-main)] overflow-hidden">
          {/* Chat panel */}
          <div
            class="flex flex-col min-w-0 h-full w-full"
            style={{ flex: showSidePanel ? "1 1 50%" : "1 1 100%" }}
          >
            <ChatContainer
              messages={chatState.messages}
              messagesEndRef={messagesEndRef}
              retry$={retry}
              canRouteOnline={selectedAi.value.aiConfig?.model === 'auto:online-offline'}
              onGround$={groundMessage$}
              scrollToBottom$={scrollToBottom}
              handleUpgradeClick$={handleUpgradeClick}
              input={input}
              handleSubmit$={handleSubmit}
              isLoading={chatState.isLoading}
              stopChat$={handleStop}
              currentPlaceholder={`Ask ${selectedAi.value.label} ${
                selectedAi.value.aiConfig?.askBlurb &&
                selectedAi.value.aiConfig.askBlurb.trim() !== ""
                  ? selectedAi.value.aiConfig.askBlurb
                  : "anything"
              }..`}
              selectedAi={selectedAi}
              dynamicModelOptions={dynamicModelOptions.value}
              currentSelectedOptionInListbox={
                currentSelectedOptionInListbox.value
              }
              selectedAction={selectedAction}
              sidePanelContent={sidePanelContent}
              isDesktop={isDesktop}
              isSidePanelVisible={isSidePanelVisible}
              theme={theme.value}
              isModelLoading={isModelLoading.value}
              currentModel={currentModel.value}
              onDownloadModel$={handleDownloadModel}
              onNewQuestion$={handleNewQuestion}
              attachedFiles={attachedFiles}
              attachedImages={attachedImages}
              contextWindowSize={contextWindowSize.value}
              onAttachFiles$={handleAttachFiles}
              onOpenFolder$={openFolder$}
              onOpenTerminal$={handleOpenTerminal}
              lastConversationTitle={
                lastConversation.value
                  ? getConversationTitleOverride(lastConversation.value.hash) ??
                    sanitizeTitle(lastConversation.value.title)
                  : undefined
              }
              onContinueLast$={$(() => {
                const last = lastConversation.value;
                if (last) resumeConversation(last);
              })}
              onPermissionRespond$={respondPermission$}
              onPermissionOffscreen$={$((off: boolean) => {
                agentState.pendingCardOffscreen = off;
              })}
              showPermissionPill={
                agentState.pendingPermissionId !== null &&
                agentState.pendingCardOffscreen
              }
              agentStreaming={
                agentState.folderPath !== null && chatState.isLoading
              }
              liveStatus={agentState.liveStatus}
              agentRetryStatus={agentState.retryStatus || undefined}
              onPermissionJump$={$(() => {
                messagesEndRef.value?.scrollIntoView({ behavior: "smooth", block: "end" });
              })}
            />

            <div class="max-w-4xl mx-auto w-full px-4">
              <OcrAttachStatus
                attachedFiles={attachedFiles}
                ocrProcessing={ocrProcessing}
                ocrNeeded={ocrNeeded}
              />
            </div>

            {chatState.error && (
              <div class="max-w-4xl mx-auto mt-4">
                {chatState.error === "NO_MODELS_AVAILABLE" ? (
                  <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                    <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                      No offline models available
                    </p>
                    <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                      To start chatting, you need to download at least one AI
                      model. Models are stored locally on your device and work
                      completely offline.
                    </p>
                    <LiquidMetalButton
                      onClick$={() => nav("/setup")}
                      class="px-4 py-2 text-sm"
                    >
                      Download Models
                    </LiquidMetalButton>
                  </div>
                ) : (() => {
                  // Online-model errors arrive as structured JSON from the
                  // proxy (relayed by llm.rs): auth_required /
                  // entitlement_required / allowance_exceeded / …
                  let online: { code?: string; message?: string } | null = null;
                  try {
                    const parsed = JSON.parse(chatState.error);
                    if (parsed && typeof parsed.code === "string") online = parsed;
                  } catch { /* not an online error */ }

                  if (online?.code === "vision_needed") {
                    const v = online as {
                      kind?: string;
                      aiLabel?: string;
                      visionModel?: string;
                    };
                    const isSwitch = v.kind === "switch";
                    // No offline vision model yet → inline download-with-progress.
                    if (!isSwitch) {
                      return (
                        <VisionDownloadCard
                          onStart$={$((files, visionModel) => {
                            const t = chatState.pendingTurn;
                            chatState.error = null;
                            chatState.pendingTurn = null;
                            visionDownload.start$(
                              files,
                              visionModel,
                              selectedAi.value.id,
                              t
                                ? {
                                    userInput: t.userInput,
                                    chatAction: t.chatAction,
                                    fileContext: t.fileContext,
                                    images: t.images,
                                  }
                                : null,
                            );
                          })}
                          onCancel$={$(() => {
                            chatState.error = null;
                            chatState.pendingTurn = null;
                          })}
                          ram={systemInfo.value?.total_memory_gb}
                          vram={systemInfo.value?.total_vram_gb ?? null}
                        />
                      );
                    }
                    // Has a vision model but the pinned AI's can't see → offer to
                    // use it just for this message.
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                          Use a vision model for this image?
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          {`${v.aiLabel ?? "This AI"}'s model can't see images, but you have one that can. Use it just for this message?`}
                        </p>
                        <div class="flex items-center gap-3">
                          <LiquidMetalButton
                            onClick$={async () => {
                              const t = chatState.pendingTurn;
                              chatState.error = null;
                              if (t) {
                                await sendMessage(
                                  t.userInput,
                                  t.chatAction,
                                  t.fileContext,
                                  t.images,
                                  t.visionModel,
                                );
                              }
                            }}
                            class="px-4 py-2 text-sm"
                          >
                            Use vision model
                          </LiquidMetalButton>
                          <button
                            onClick$={() => {
                              chatState.error = null;
                              chatState.pendingTurn = null;
                            }}
                            class="text-sm text-yellow-700 dark:text-yellow-300 hover:underline"
                          >
                            Not now
                          </button>
                        </div>
                      </div>
                    );
                  }

                  if (online?.code === "cloud_consent") {
                    const c = online as {
                      onlineLabel?: string;
                      hasImage?: boolean;
                      hasDoc?: boolean;
                    };
                    const what =
                      c.hasImage && c.hasDoc
                        ? "image and file"
                        : c.hasImage
                          ? "image"
                          : "file";
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                          Send this {what} to an online model?
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          {c.onlineLabel ?? "This AI"} uses an online model, so your
                          {" "}{what} would leave your device and be sent to the cloud
                          to answer. Offline models keep everything on your device.
                        </p>
                        <div class="flex items-center gap-3 flex-wrap">
                          <LiquidMetalButton
                            onClick$={async () => {
                              const t = chatState.pendingTurn;
                              chatState.error = null;
                              if (t)
                                await sendMessage(t.userInput, t.chatAction, t.fileContext, t.images, undefined, true);
                            }}
                            class="px-4 py-2 text-sm"
                          >
                            Send once
                          </LiquidMetalButton>
                          <button
                            onClick$={async () => {
                              localStorage.setItem("allowAttachmentsOnline", "true");
                              const t = chatState.pendingTurn;
                              chatState.error = null;
                              if (t)
                                await sendMessage(t.userInput, t.chatAction, t.fileContext, t.images, undefined, true);
                            }}
                            class="text-sm text-yellow-700 dark:text-yellow-300 hover:underline"
                          >
                            Always allow
                          </button>
                          <button
                            onClick$={() => {
                              chatState.error = null;
                              chatState.pendingTurn = null;
                            }}
                            class="text-sm text-yellow-700 dark:text-yellow-300 hover:underline"
                          >
                            Cancel
                          </button>
                        </div>
                      </div>
                    );
                  }

                  if (online?.code === "auth_required") {
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                          Sign in to use online models
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          Online models are powered by your Flowsta account,
                          via Flowsta Vault on this device. Local models keep
                          working without it.
                        </p>
                        <LiquidMetalButton onClick$={() => nav("/settings")} class="px-4 py-2 text-sm">
                          Go to Settings
                        </LiquidMetalButton>
                      </div>
                    );
                  }
                  if (online?.code === "entitlement_required") {
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                          Online models need a plan
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          This device isn't linked to a Your Own AI plan yet.
                          Link it (or subscribe) in the browser — it takes a
                          few seconds — then try again.
                        </p>
                        <LiquidMetalButton
                          onClick$={async () => {
                            try {
                              const { invoke } = await import("@tauri-apps/api/core");
                              const { openUrl } = await import("@tauri-apps/plugin-opener");
                              const url = await invoke<string>("flowsta_link_url");
                              await openUrl(url);
                            } catch {
                              nav("/settings");
                            }
                          }}
                          class="px-4 py-2 text-sm"
                        >
                          Link my plan
                        </LiquidMetalButton>
                      </div>
                    );
                  }
                  if (online?.code === "allowance_exceeded" || online?.code === "overage_settlement_failed") {
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3">
                          {online.code === "allowance_exceeded"
                            ? "You've reached your monthly allowance"
                            : "There's a billing issue"}
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          {online.message ||
                            "Manage your plan in the browser to keep going."}
                        </p>
                        <LiquidMetalButton
                          onClick$={async () => {
                            const { invoke } = await import("@tauri-apps/api/core");
                            const { openUrl } = await import("@tauri-apps/plugin-opener");
                            const url = await invoke<string>("flowsta_link_url").catch(() => null);
                            await openUrl(url ? url.split("?")[0] : "https://yourownai.net/account/");
                          }}
                          class="px-4 py-2 text-sm"
                        >
                          Manage plan
                        </LiquidMetalButton>
                      </div>
                    );
                  }
                  if (online?.code === "online_unreachable") {
                    const o = online as {
                      offline?: boolean;
                      canSwitchAuto?: boolean;
                      aiId?: string;
                      aiLabel?: string;
                    };
                    return (
                      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
                        <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-1">
                          {o.offline ? "You're offline" : "Can't reach the online service"}
                        </p>
                        <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-4">
                          {o.offline
                            ? `${o.aiLabel} runs on an online model, so it needs an internet connection.${o.canSwitchAuto ? " Switch it to Auto and it'll answer offline now, and use the web again when you're back online." : ""}`
                            : "The online-model service is unreachable right now. Check your connection, or try again in a moment."}
                        </p>
                        <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2">
                          <LiquidMetalButton
                            variant="secondary"
                            onClick$={async () => {
                              const t = chatState.pendingTurn;
                              chatState.pendingTurn = null;
                              chatState.error = null;
                              if (t) await sendMessage(t.userInput, t.chatAction, t.fileContext, t.images);
                            }}
                            class="px-4 py-2 text-sm"
                          >
                            Try again
                          </LiquidMetalButton>
                          {o.canSwitchAuto && (
                            <LiquidMetalButton
                              onClick$={async () => {
                                try {
                                  if (o.aiId) {
                                    await updateCustomAi(o.aiId, { model: "auto:online-offline" });
                                    selectedAi.value = {
                                      ...selectedAi.value,
                                      aiConfig: {
                                        ...selectedAi.value.aiConfig,
                                        model: "auto:online-offline",
                                      },
                                    };
                                  }
                                } catch (e) {
                                  console.warn("[Chat] switch-to-auto failed:", e);
                                }
                                const t = chatState.pendingTurn;
                                chatState.pendingTurn = null;
                                chatState.error = null;
                                if (t) await sendMessage(t.userInput, t.chatAction, t.fileContext, t.images);
                              }}
                              class="px-4 py-2 text-sm"
                            >
                              Switch {o.aiLabel} to Auto
                            </LiquidMetalButton>
                          )}
                        </div>
                      </div>
                    );
                  }
                  return (
                    <div class="text-red-500 p-2 rounded bg-red-100 dark:bg-red-900 dark:text-red-300">
                      Error: {online?.message || chatState.error}
                    </div>
                  );
                })()}
              </div>
            )}
          </div>

          {/* Side panel (code preview) — simple CSS flex layout with draggable divider */}
          {showSidePanel && sidePanelContent.value && (
            <>
              <div
                class="w-1 bg-[var(--border-subtle)] hover:bg-[var(--border-interactive)] transition-colors cursor-col-resize shrink-0"
                aria-label="Resize handle"
              />
              <div
                class="border-l border-[var(--border-subtle)] overflow-y-auto h-full"
                style={{ flex: "1 1 50%" }}
              >
                <CodePanel
                  codeString={sidePanelContent.value.codeString}
                  language={sidePanelContent.value.language}
                  theme={theme.value}
                  // The panel's own X - same effect as the message's Hide
                  // Code button, so either door closes it and Show Code
                  // reopens it.
                  onClose$={() => {
                    isSidePanelVisible.value = false;
                  }}
                  onOpenTerminal$={handleOpenTerminal}
                />
              </div>
            </>
          )}
        </main>
      </div>

      {/* Conversations drawer - every conversation is a place. */}
      <ConversationsDrawer
        open={conversationsOpen.value}
        items={conversationItems.value}
        loading={conversationsLoading.value}
        onClose$={$(() => {
          conversationsOpen.value = false;
        })}
        onResume$={$((item: ConversationListItem) =>
          resumeConversation({
            hash: item.conversation.hash,
            agentKey: item.conversation.agent_key,
            aiId: item.aiId,
          }),
        )}
        onRename$={$((hash: string, title: string) => {
          setConversationTitleOverride(hash, title);
          conversationItems.value = conversationItems.value.map((i) =>
            i.conversation.hash === hash ? { ...i, title } : i,
          );
          lastConversation.value = readLastConversation();
        })}
      />

      {/* Opening a project without the Build add-on: one modal, one
          download, background install - the header icon shows progress. */}
      <ConfirmModal
        isOpen={installAsk.value}
        title="Projects need Your Own AI Build"
        message="Your AI can work in project folders once Your Own AI Build is installed - a free add-on, about 50 MB. It downloads in the background while you keep chatting; the project icon in the header shows progress."
        confirmLabel="Download now"
        cancelLabel="Not now"
        onConfirm$={async () => {
          installAsk.value = false;
          try {
            invoke("download_build_agent").catch(() => {});
          } catch {
            /* the header surfaces failures */
          }
        }}
        onCancel$={() => {
          installAsk.value = false;
        }}
      />

      {/* Folder guard: the workspace opened, but this AI's model is a poor
          fit for agent work - offer the switch, never substitute quietly. */}
      <ConfirmModal
        isOpen={folderGuard.value !== null}
        title={`${folderGuard.value?.currentLabel ?? "This AI"} may struggle with project work`}
        message={
          folderGuard.value?.suggestion
            ? `${folderGuard.value.currentLabel}'s model isn't built for tool work, so project tasks may stall or fail. ${folderGuard.value.suggestion.label} can drive them properly - switch this project to ${folderGuard.value.suggestion.label}?`
            : `${folderGuard.value?.currentLabel ?? "This AI"}'s model isn't built for tool work, so project tasks may stall or fail. None of your other AIs are set up for it yet either - an online model like Kimi, or an agentic coder from the model library, works best.`
        }
        confirmLabel={
          folderGuard.value?.suggestion
            ? `Use ${folderGuard.value.suggestion.label}`
            : "OK"
        }
        cancelLabel={
          folderGuard.value?.suggestion
            ? `Keep ${folderGuard.value.currentLabel}`
            : "Keep going"
        }
        onConfirm$={async () => {
          const guard = folderGuard.value;
          folderGuard.value = null;
          if (guard?.suggestion) {
            selectedAi.value = guard.suggestion;
            await openFolder$(guard.path);
          }
        }}
        onCancel$={() => {
          folderGuard.value = null;
        }}
      />

      {/* An agent turn died on an overloaded online model: offer the next
          capable model for this session - an explicit switch, never a silent
          reroute. Accepting re-asks the failed question on the new model;
          the Agent setting itself stays unchanged. */}
      <ConfirmModal
        isOpen={agentState.overloadOffer !== null}
        title={`${agentState.overloadOffer?.failedName ?? "The model"} is overloaded right now`}
        message={`${agentState.overloadOffer?.failedName ?? "The model"}'s provider is refusing requests at the moment. Use ${agentState.overloadOffer?.altName ?? "another capable model"} for the rest of this session and ask again? Your Agent model setting stays as it is.`}
        confirmLabel={`Use ${agentState.overloadOffer?.altName ?? "the alternative"}`}
        cancelLabel="Not now"
        onConfirm$={async () => {
          await acceptOverloadOffer$();
        }}
        onCancel$={async () => {
          await dismissOverloadOffer$();
        }}
      />

      {/* Resumed a conversation that worked in a folder: ask before
          switching the workspace - never silently swap it. */}
      <ConfirmModal
        isOpen={resumeFolderAsk.value !== null}
        title="Open this conversation's project?"
        message={`This conversation worked in ${
          resumeFolderAsk.value?.split("/").filter(Boolean).pop() ?? "a folder"
        }. Open that project to keep building?`}
        confirmLabel="Open project"
        cancelLabel="Just read"
        onConfirm$={async () => {
          const folder = resumeFolderAsk.value;
          resumeFolderAsk.value = null;
          if (!folder) return;
          if (!buildInstalled.value) {
            installAsk.value = true;
            return;
          }
          await openFolder$(folder);
        }}
        onCancel$={() => {
          resumeFolderAsk.value = null;
        }}
      />

      {/* Closing the folder mid-task is the one destructive gesture here -
          confirm it. Idle closes never see this. */}
      <ConfirmModal
        isOpen={showCloseFolderConfirm.value}
        title="Stop the agent?"
        message="The agent is still working. Stop it and close the project? Changes already made stay on disk."
        confirmLabel="Stop & close"
        cancelLabel="Keep working"
        variant="danger"
        onConfirm$={async () => {
          showCloseFolderConfirm.value = false;
          await closeFolder$();
        }}
        onCancel$={() => {
          showCloseFolderConfirm.value = false;
        }}
      />

      {/* Welcome Modal — first-run model recommendation and download */}
      {showWelcomeModal.value && (
        <WelcomeModal
          isOpen={showWelcomeModal.value}
          onClose$={() => {
            needsWelcomeModal = false;
            showWelcomeModal.value = false;
          }}
          systemInfo={systemInfo.value}
          onModelDownloaded$={handleModelDownloaded}
        />
      )}
    </div>
  );
});

export const head: DocumentHead = {
  title: "Chat - Your Own AI",
};
