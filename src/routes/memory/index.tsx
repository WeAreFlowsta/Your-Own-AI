/**
 * Memory Page — Shows an AI's conversation history from Holochain.
 *
 * Each conversation is a tamper-proof record on the AI's source chain.
 * Tapping a conversation shows the full transcript.
 */

import {
  component$,
  useContext,
  useSignal,
  useTask$,
  useVisibleTask$,
  $,
  type Signal,
} from "@builder.io/qwik";
import { LuFolderOpen } from "@qwikest/icons/lucide";
import {
  listWorkspaceMemories,
  type WorkspaceMemory,
} from "../../utils/workspaceMemory";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuArrowLeft, LuMessageSquare, LuChevronDown, LuChevronUp, LuInfo, LuDownload, LuPencil, LuShieldCheck, LuBrain, LuUser, LuTrash2 } from "@qwikest/icons/lucide";
import ConfirmModal from "../../components/ConfirmModal";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import { ProjectMemoryContext } from "../layout";
import { useAiData } from "../../contexts/AiDataContext";
import {
  getConversations,
  getTranscript,
} from "../../utils/holochainTranscripts";
import {
  sanitizeTitle,
  setConversationTitleOverride,
  getConversationTitleOverride,
} from "../../utils/conversationResume";
import type {
  HolochainConversation,
  HolochainTranscriptEntry,
} from "../../types";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { renderMarkdown } from "../../utils/renderMarkdown";
import { exportConversation, describeSignError, type ExportOptions } from "../../utils/exportConversation";
import { ExportConversationModal } from "../../components/ExportConversationModal";
import ProfileMemory from "../../components/ProfileMemory";
import AiEpisodicMemory from "../../components/AiEpisodicMemory";
import AiKnowledge from "../../components/AiKnowledge";
import { RememberEntryButton } from "../../components/RememberEntryButton";
import AiKnowledgeDocuments from "../../components/AiKnowledgeDocuments";
import { Callout } from "../../components/Callout";
import { emptyMayBeWarmup, noteRecordsSeen, readThroughWarmup, WARMUP_POLL_MS } from "../../utils/recordsWarmup";

/**
 * Header subtitle in its own component so the signal reads get a clean
 * reactive scope — reading these directly in the page header left the text
 * node frozen at first render (showed "0" / "Loading…" forever) while the
 * content area updated normally.
 */
const MemorySubtitle = component$<{
  loading: Signal<boolean>;
  warming: Signal<boolean>;
  conversations: Signal<HolochainConversation[]>;
}>(({ loading, warming, conversations }) => {
  const count = conversations.value.length;
  return (
    <p class="text-sm text-[var(--text-secondary)] flex items-center gap-1.5">
      <LuShieldCheck class="w-3.5 h-3.5 text-emerald-400 flex-shrink-0" />
      {loading.value
        ? "Loading…"
        : warming.value
          ? "Your records are warming up…"
          : `${count} tamper-proof conversation${count !== 1 ? "s" : ""}`}
    </p>
  );
});

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();

  const aiId = useSignal("");
  const agentKey = useSignal("");
  const conversations = useSignal<HolochainConversation[]>([]);
  // "Warming": the records layer answered EMPTY inside the launch grace
  // window - not believable as zero yet (see utils/recordsWarmup). Shown
  // as its own state, never as "0" or "No memories yet".
  const warming = useSignal(false);
  const loading = useSignal(true);
  const expandedHash = useSignal<string | null>(null);
  const transcriptEntries = useSignal<HolochainTranscriptEntry[]>([]);
  // Rename (client-side override until the zome gains a rename fn).
  const renamingHash = useSignal<string | null>(null);
  const renameDraft = useSignal("");
  // Bumped after a rename so the title expressions re-read the override.
  const titleBump = useSignal(0);
  const transcriptLoading = useSignal(false);
  const transcriptError = useSignal(false);

  // AI name and thumbnail — updated when aiData loads
  const aiName = useSignal("");
  const thumbUrl = useSignal<string | null>(null);

  // Read AI id and agent key from sessionStorage (query params are stripped in static builds)
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    aiId.value = sessionStorage.getItem("memory-ai-id") || "";
    agentKey.value = sessionStorage.getItem("memory-agent-key") || "";
    aiName.value = aiId.value;
  }, { strategy: 'document-ready' });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const id = track(() => aiId.value);
    const ais = track(() => aiData.userDefinedAis);
    const templates = track(() => aiData.archetypeTemplates);
    const thumbs = track(() => aiData.thumbnailObjectUrls);

    if (!id) return;

    const userAi = (ais || []).find((a: any) => a.id === id);
    if (userAi) {
      aiName.value = userAi.name;
      // If the user clicked Memory before provisioning finished,
      // sessionStorage holds an empty agent key ("agentPubKey || ''").
      // Resolve it from context once provisioning lands so the page
      // doesn't wait forever.
      if (!agentKey.value && (userAi as any).agentPubKey) {
        agentKey.value = (userAi as any).agentPubKey;
        sessionStorage.setItem("memory-agent-key", agentKey.value);
      }
    } else {
      const template = (templates || []).find((a: any) => a.id === id);
      aiName.value = template?.name || id;
    }
    thumbUrl.value = thumbs?.[id] || null;
  }, { strategy: 'document-ready' });

  // Load conversations — waits for agent key and AiDataContext
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const key = track(() => agentKey.value);
    const isReady = track(() => aiData.isInitialized);
    if (!isReady) return;
    if (!key) {
      // Context is initialized but no agent key could be resolved (AI
      // never provisioned) — show the empty state rather than spinning
      // forever. If the key resolves later, track() re-runs this task.
      loading.value = false;
      return;
    }

    loading.value = true;
    warming.value = false;
    // Retry a few times in case conductor is still starting - and keep
    // polling through the warmup window when the read SUCCEEDS but empty
    // (the cell is up before its records are; an early empty is "not
    // yet", not "none").
    for (let attempt = 0; ; attempt++) {
      try {
        const result = await getConversations(key);
        if (result.length > 0) noteRecordsSeen();
        conversations.value = result;
        loading.value = false;
        if (result.length === 0 && emptyMayBeWarmup()) {
          warming.value = true;
          await new Promise((r) => setTimeout(r, WARMUP_POLL_MS));
          continue;
        }
        warming.value = false;
        return;
      } catch (e) {
        if (attempt < 4) {
          await new Promise((r) => setTimeout(r, 2000));
        } else {
          console.warn("[Memory] Failed to load conversations after retries:", e);
          loading.value = false;
          warming.value = false;
          return;
        }
      }
    }
  }, { strategy: 'document-ready' });

  const loadTranscript = $(async (hash: string, convAgentKey?: string) => {
    transcriptLoading.value = true;
    transcriptError.value = false;
    try {
      // Route the read to the agent generation that holds this
      // conversation (older conversations live on earlier agents).
      // Bound the wait — a busy/stuck conductor can otherwise leave the
      // zome read pending forever (infinite spinner).
      const entries = await Promise.race([
        getTranscript(convAgentKey || agentKey.value, hash),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("transcript read timed out")), 25000),
        ),
      ]);
      transcriptEntries.value = entries;
    } catch (e) {
      console.warn("[Memory] Failed to load transcript:", e);
      transcriptEntries.value = [];
      transcriptError.value = true;
    } finally {
      transcriptLoading.value = false;
    }
  });

  const toggleConversation = $(async (hash: string, convAgentKey?: string) => {
    if (expandedHash.value === hash) {
      expandedHash.value = null;
      transcriptEntries.value = [];
      transcriptError.value = false;
      return;
    }
    expandedHash.value = hash;
    await loadTranscript(hash, convAgentKey);
  });

  const formatDate = (timestamp: number) => {
    // Holochain timestamps are in microseconds
    const ms = timestamp > 1e15 ? timestamp / 1000 : timestamp;
    return new Date(ms).toLocaleString();
  };

  const shortHash = (h: string) =>
    h && h.length > 16 ? `${h.slice(0, 8)}…${h.slice(-6)}` : h;

  // Brief inline status for exports (saved path / errors).
  const exportStatus = useSignal<string | null>(null);
  // Export-options modal: which conversation, and whether it's open.
  const exportModalConv = useSignal<HolochainConversation | null>(null);
  const deleteModalConv = useSignal<HolochainConversation | null>(null);
  const deletingConv = useSignal(false);
  // A failed delete says so where the row was - never a silent no-op.
  const deleteError = useSignal<string | null>(null);

  const handleExport = $(async (conv: HolochainConversation, opts: ExportOptions, sign: boolean) => {
    exportStatus.value = sign
      ? "Signing in your Flowsta Vault - approve the request there…"
      : "Exporting conversation…";
    try {
      const entries =
        expandedHash.value === conv.hash && transcriptEntries.value.length
          ? transcriptEntries.value
          : await getTranscript(conv.agent_key || agentKey.value, conv.hash);
      const res = await exportConversation(conv, entries, aiName.value, opts, sign);
      if (res.signed) {
        exportStatus.value = `Saved to ${res.path} - signed and published to Sign It. Anyone can verify this exact file at flowsta.com/sign-it.`;
      } else if (res.signError) {
        exportStatus.value = `Saved to ${res.path} (unsigned) - signing failed: ${describeSignError(res.signError)}`;
      } else {
        exportStatus.value = `Saved to ${res.path}`;
      }
    } catch (e) {
      exportStatus.value = `Export failed: ${e}`;
    }
  });

  // Tabs: Conversations (the per-AI ledger, default) | Knows (read-only
  // mirror) | Workspaces (per-folder memory, shared by all AIs).
  const activeTab = useSignal<"knows" | "conversations" | "workspaces">("conversations");
  const workspaceMemories = useSignal<WorkspaceMemory[]>([]);
  const workspacesLoading = useSignal(false);
  // The modal renders at the layout root (route stacking contexts
  // would bury it) - this shared signal opens it.
  const memoryFolder = useContext(ProjectMemoryContext);

  const workspacesWarming = useSignal(false);
  // useVisibleTask$, NOT useTask$ (gotcha #21): a tracked useTask$ that
  // awaits blocks the component's re-renders until it settles - and
  // readThroughWarmup deliberately polls for minutes during records
  // warmup, which froze every tab switch on this page.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track, cleanup }) => {
    const tab = track(() => activeTab.value);
    const reopened = track(() => memoryFolder.value);
    if (tab !== "workspaces" || reopened) return;
    let alive = true;
    cleanup(() => (alive = false));
    workspacesLoading.value = true;
    try {
      // Project memories read through the conductor - an early empty is
      // "not yet", not "none" (readThroughWarmup holds the line).
      workspaceMemories.value = await readThroughWarmup(
        listWorkspaceMemories,
        (w) => (workspacesWarming.value = w),
        () => alive,
      );
    } finally {
      workspacesLoading.value = false;
    }
  });

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <AppHeader
        currentModel={null}
        handleNewQuestion$={$(() => nav("/chat/"))}
        handleModelsClick$={$(() => nav("/setup/"))}
        folderPath={headerWs.folderPath.value}
        folderStatus={headerWs.folderStatus.value}
        permissionMode={headerWs.permissionMode.value}
        onCloseFolder$={headerWs.closeFolder$}
        buildInstalled={headerWs.buildInstalled.value}
        recentFolders={headerWs.recentFolders.value}
        onOpenFolder$={headerWs.openFolder$}
        onBrowseFolder$={headerWs.browseFolder$}
        onOpenConversations$={headerWs.openConversations$}
      />

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-5xl mx-auto px-5 py-8">
          {/* Header */}
          <div class="flex items-center gap-3 mb-6">
            <LiquidMetalButton
              onClick$={() => nav("/your-ais/")}
              class="p-2"
            >
              <LuArrowLeft class="w-5 h-5" />
            </LiquidMetalButton>

            {thumbUrl.value && thumbUrl.value !== "error" ? (
              <img
                src={thumbUrl.value}
                alt={aiName.value}
                class="w-11 h-11 rounded-full object-cover ring-1 ring-[var(--border-subtle)]"
                width={44}
                height={44}
              />
            ) : (
              <div class="w-11 h-11 rounded-full bg-gradient-to-br from-purple-500 to-indigo-600 flex items-center justify-center text-white font-bold text-lg">
                {aiName.value.charAt(0).toUpperCase()}
              </div>
            )}

            <div class="min-w-0 flex-1">
              <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela leading-tight">
                {aiName.value}'s Memory
              </h2>
              <MemorySubtitle loading={loading} warming={warming} conversations={conversations} />
            </div>

          </div>

          {/* Tabs */}
          <div class="flex items-center gap-1 mb-6 border-b border-[var(--border-subtle)]">
            <button
              onClick$={() => (activeTab.value = "conversations")}
              class={`flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
                activeTab.value === "conversations"
                  ? "border-[var(--bg-button-primary)] text-[var(--text-primary)]"
                  : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              <LuMessageSquare class="w-4 h-4" />
              Conversations
            </button>
            <button
              onClick$={() => (activeTab.value = "workspaces")}
              class={`flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
                activeTab.value === "workspaces"
                  ? "border-[var(--bg-button-primary)] text-[var(--text-primary)]"
                  : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              <LuFolderOpen class="w-4 h-4" />
              Projects
            </button>
            <button
              onClick$={() => (activeTab.value = "knows")}
              class={`flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
                activeTab.value === "knows"
                  ? "border-[var(--bg-button-primary)] text-[var(--text-primary)]"
                  : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              <LuBrain class="w-4 h-4" />
              Remembers
            </button>
          </div>

          {/* Export status */}
          {exportStatus.value && (
            <div class="mb-5 flex items-start justify-between gap-3 p-3 rounded-lg bg-[var(--bg-card)] border border-[var(--border-subtle)] text-xs text-[var(--text-secondary)]">
              <span class="break-all">{exportStatus.value}</span>
              <button
                class="flex-shrink-0 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                onClick$={() => (exportStatus.value = null)}
              >
                ✕
              </button>
            </div>
          )}

          {deleteError.value && (
            <div class="mb-5 flex items-start justify-between gap-3 p-3 rounded-lg bg-red-500/10 border border-red-500/25 text-xs text-[var(--text-secondary)]">
              <span>{deleteError.value}</span>
              <button
                class="flex-shrink-0 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                onClick$={() => (deleteError.value = null)}
              >
                ✕
              </button>
            </div>
          )}

          {/* Knows tab — the shared profile, read-only here; managed on Your Memory */}
          {/* Workspaces tab - per-folder memory shared by every AI. */}
          {activeTab.value === "workspaces" && (
            <div>
              <p class="text-sm text-[var(--text-secondary)] mb-4">
                What your AIs remember about each project they have worked on -
                commands, conventions, decisions. Kept in your records, shared
                by all your AIs, yours to edit.
              </p>
              {workspacesLoading.value && (
                <p class="text-sm text-[var(--text-muted)]">
                  {workspacesWarming.value
                    ? "Your records are warming up - just after launch, project memories take a moment to be ready."
                    : "Loading from your records.."}
                </p>
              )}
              {!workspacesLoading.value && workspaceMemories.value.length === 0 && (
                <p class="text-sm text-[var(--text-muted)]">
                  Nothing yet - project memory grows as your AIs work on
                  projects.
                </p>
              )}
              {workspaceMemories.value.map((w) => (
                <button
                  key={w.folderPath}
                  onClick$={() => (memoryFolder.value = w.folderPath)}
                  class="flex w-full items-center gap-3 px-4 py-3 mb-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] hover:bg-[var(--bg-dropdown-hover)] text-left"
                >
                  <LuFolderOpen class="h-4 w-4 shrink-0 opacity-60" />
                  <span class="min-w-0 flex-1">
                    <span class="block text-sm text-[var(--text-primary)] truncate">
                      {w.folderPath.split("/").filter(Boolean).pop()}
                    </span>
                    <span class="block text-xs text-[var(--text-muted)] truncate">
                      {w.folderPath}
                    </span>
                  </span>
                  <span class="shrink-0 text-xs text-[var(--text-muted)]">
                    {w.revisions} revision{w.revisions === 1 ? "" : "s"}
                  </span>
                </button>
              ))}
            </div>
          )}

          {activeTab.value === "knows" && (
            <div class="space-y-8">
              <div>
                <Callout intent="premium" title="Knowledge you give it" id="memory-tip-knowledge" class="mb-4">
                  Give {aiName.value || "this AI"} knowledge only it should have —
                  a character's backstory and world, or your product details,
                  policies, and procedures. It draws on them when they're
                  relevant, and you can export the set as a shareable pack.
                </Callout>
                <AiKnowledge aiId={aiId.value} aiName={aiName.value} />
              </div>
              <div class="border-t border-[var(--border-subtle)] pt-6">
                <Callout intent="info" title="Documents you've given it" id="memory-tip-documents" class="mb-4">
                  Files you add to {aiName.value || "this AI"} from the Knowledge
                  tab when editing it. It reads and remembers them, then uses the
                  relevant parts whenever they fit — the original files can be
                  moved or deleted after.
                </Callout>
                <AiKnowledgeDocuments aiId={aiId.value} aiName={aiName.value} />
              </div>
              <div class="border-t border-[var(--border-subtle)] pt-6">
                <Callout intent="info" title="What it remembers" id="memory-tip-episodic" class="mb-4">
                  As you chat, {aiName.value || "this AI"} quietly remembers key
                  moments from your conversations and brings them back when they
                  fit — kept just for this AI, separate from your others.
                </Callout>
                <AiEpisodicMemory aiId={aiId.value} aiName={aiName.value} />
              </div>
              <div class="border-t border-[var(--border-subtle)] pt-6">
                <Callout intent="info" title="Shared across your AIs" id="memory-tip-shared" class="mb-4">
                  Facts about you — your name, preferences, projects — that every
                  AI uses, so you don't repeat yourself. Manage them on the Your
                  Memory page.
                </Callout>
                <ProfileMemory readOnly manageHref="/your-memory/" />
              </div>
            </div>
          )}

          {/* Conversations tab */}
          {activeTab.value === "conversations" && (
            <Callout intent="info" title="Tamper-proof conversations" id="memory-tip-transcripts" class="mb-4">
              Every conversation here is cryptographically signed (using
              Holochain) as it's saved — so the record can't be quietly changed
              or faked later, and you can trust it's exactly what was said. It
              stays private and encrypted on your device.
            </Callout>
          )}


          {activeTab.value === "conversations" &&
            (loading.value ? (
            <div class="text-center py-16">
              <div class="inline-block w-8 h-8 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
              <p class="mt-4 text-[var(--text-secondary)]">Loading memories...</p>
            </div>
          ) : warming.value && conversations.value.length === 0 ? (
            <div class="text-center py-16">
              <div class="inline-block w-8 h-8 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
              <p class="mt-4 text-[var(--text-primary)]">Your records are warming up</p>
              <p class="mt-1 text-sm text-[var(--text-secondary)]">
                Just after launch, your conversations take a moment to be
                ready.
              </p>
            </div>
          ) : conversations.value.length === 0 ? (
            <div class="text-center py-16 rounded-2xl border border-dashed border-[var(--border-subtle)]">
              <LuMessageSquare class="w-12 h-12 mx-auto mb-3 text-[var(--text-muted)]" />
              <p class="text-lg text-[var(--text-primary)]">No memories yet</p>
              <p class="text-sm mt-1 text-[var(--text-secondary)]">
                Start a conversation with {aiName.value} to create memories.
              </p>
            </div>
          ) : (
            <div class="space-y-3">
              {conversations.value.map((conv) => (
                <div
                  key={conv.hash}
                  class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] overflow-hidden transition-colors hover:border-[var(--border-primary)]/40"
                >
                  {/* Conversation header */}
                  <div class="flex items-center gap-1 pr-2">
                    <button
                      class="flex-1 flex items-center gap-3 min-w-0 p-4 text-left"
                      onClick$={() => toggleConversation(conv.hash, conv.agent_key)}
                    >
                      <div class="w-9 h-9 flex-shrink-0 rounded-lg bg-[var(--bg-main)] flex items-center justify-center">
                        <LuMessageSquare class="w-[18px] h-[18px] text-[var(--bg-button-primary)]" />
                      </div>
                      <div class="min-w-0">
                        <div class="font-medium text-[var(--text-primary)] truncate">
                          {renamingHash.value === conv.hash ? (
                            <input
                              type="text"
                              value={renameDraft.value}
                              autoFocus
                              onClick$={(e) => e.stopPropagation()}
                              onInput$={(_, el) => (renameDraft.value = el.value)}
                              onKeyDown$={(e) => {
                                if (e.key === "Enter" || e.key === "Escape") {
                                  if (e.key === "Enter" && renameDraft.value.trim()) {
                                    setConversationTitleOverride(conv.hash, renameDraft.value.trim());
                                    titleBump.value++;
                                  }
                                  renamingHash.value = null;
                                }
                              }}
                              onBlur$={() => {
                                if (renameDraft.value.trim()) {
                                  setConversationTitleOverride(conv.hash, renameDraft.value.trim());
                                  titleBump.value++;
                                }
                                renamingHash.value = null;
                              }}
                              class="w-full bg-[var(--bg-input)] border border-[var(--border-input)] rounded-lg px-2 py-1 text-sm"
                            />
                          ) : (
                            <>
                              {titleBump.value >= 0
                                ? getConversationTitleOverride(conv.hash) ??
                                  sanitizeTitle(conv.title)
                                : ""}
                            </>
                          )}
                        </div>
                        <div class="mt-0.5 flex items-center gap-2 flex-wrap text-[11px] text-[var(--text-muted)]">
                          <span>{formatDate(conv.started_at)}</span>
                          <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-secondary)] font-mono">{conv.model_used}</span>
                          {conv.source && (
                            <span
                              title={
                                conv.source.startsWith("import:")
                                  ? `Imported from your ${conv.source.slice(7)} export`
                                  : `Via an external app: ${conv.source}`
                              }
                              class="px-1.5 py-0.5 rounded bg-[var(--text-link)]/15 text-[var(--text-link)] font-semibold tracking-wide"
                            >
                              {conv.source.startsWith("import:") ? "Imported" : "API"}
                            </span>
                          )}
                        </div>
                      </div>
                    </button>
                    <button
                      onClick$={() => {
                        renameDraft.value =
                          getConversationTitleOverride(conv.hash) ??
                          sanitizeTitle(conv.title);
                        renamingHash.value = conv.hash;
                      }}
                      title="Rename this conversation"
                      class="flex-shrink-0 p-2 rounded-lg text-[var(--text-muted)] hover:bg-[var(--bg-main)] hover:text-[var(--text-primary)] transition-colors"
                    >
                      <LuPencil class="w-4 h-4" />
                    </button>
                    <button
                      onClick$={() => (exportModalConv.value = conv)}
                      title="Export this conversation as a shareable receipt (Markdown, with grounded sources)"
                      class="flex-shrink-0 p-2 rounded-lg text-[var(--text-muted)] hover:bg-[var(--bg-main)] hover:text-[var(--text-primary)] transition-colors"
                    >
                      <LuDownload class="w-4 h-4" />
                    </button>
                    <button
                      onClick$={() => (deleteModalConv.value = conv)}
                      title="Delete this conversation from your records"
                      class="flex-shrink-0 p-2 rounded-lg text-[var(--text-muted)] hover:bg-[var(--bg-main)] hover:text-red-400 transition-colors"
                    >
                      <LuTrash2 class="w-4 h-4" />
                    </button>
                    <button
                      class="flex-shrink-0 p-2 text-[var(--text-muted)]"
                      onClick$={() => toggleConversation(conv.hash, conv.agent_key)}
                    >
                      {expandedHash.value === conv.hash ? (
                        <LuChevronUp class="w-5 h-5" />
                      ) : (
                        <LuChevronDown class="w-5 h-5" />
                      )}
                    </button>
                  </div>

                  {/* Expanded transcript */}
                  {expandedHash.value === conv.hash && (
                    <div class="border-t border-[var(--border-subtle)] px-4 py-4 space-y-5">
                      {/* Every view offers re-entry - a conversation is a
                          place, not a document. Handoff via sessionStorage
                          (query params are unreliable in the packaged app). */}
                      <div class="flex justify-end">
                        <button
                          onClick$={() => {
                            try {
                              sessionStorage.setItem(
                                "resume-conversation",
                                JSON.stringify({
                                  hash: conv.hash,
                                  agentKey: conv.agent_key,
                                }),
                              );
                            } catch {
                              /* handoff unavailable */
                            }
                            nav("/chat/");
                          }}
                          class="px-3 py-1.5 rounded-lg text-xs text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] transition-colors"
                        >
                          Continue this conversation
                        </button>
                      </div>
                      {transcriptLoading.value ? (
                        <div class="flex justify-center py-4">
                          <div class="w-5 h-5 border-2 border-[var(--bg-button-primary)] border-t-transparent rounded-full animate-spin" />
                        </div>
                      ) : transcriptError.value ? (
                        <div class="py-2 flex items-center justify-between gap-3">
                          <p class="text-sm text-[var(--text-muted)]">
                            Couldn't load this transcript — the conductor may still be starting or busy.
                          </p>
                          <button
                            onClick$={() => loadTranscript(conv.hash, conv.agent_key)}
                            class="flex-shrink-0 px-3 py-1.5 rounded-lg text-xs text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] transition-colors"
                          >
                            Retry
                          </button>
                        </div>
                      ) : transcriptEntries.value.length === 0 ? (
                        <p class="text-sm text-[var(--text-muted)] py-2">No messages recorded.</p>
                      ) : (
                        transcriptEntries.value.map((entry, i) => (
                          <div key={`${conv.hash}-${i}`} class="text-sm">
                            {/* Row header: who · when · verified hash */}
                            <div class="flex items-center gap-2 mb-1.5">
                              {entry.role === "user" ? (
                                <div class="w-6 h-6 rounded-full bg-[var(--bg-button-primary)]/15 flex items-center justify-center flex-shrink-0">
                                  <LuUser class="w-3.5 h-3.5 text-[var(--bg-button-primary)]" />
                                </div>
                              ) : thumbUrl.value && thumbUrl.value !== "error" ? (
                                <img src={thumbUrl.value} alt="" class="w-6 h-6 rounded-full object-cover flex-shrink-0" width={24} height={24} />
                              ) : (
                                <div class="w-6 h-6 rounded-full bg-gradient-to-br from-emerald-400 to-teal-500 flex-shrink-0" />
                              )}
                              <span class="font-medium text-[var(--text-primary)]">
                                {entry.role === "user" ? "You" : aiName.value}
                              </span>
                              <span class="text-[11px] text-[var(--text-muted)]">{formatDate(entry.timestamp)}</span>
                              <span
                                class="ml-auto flex items-center gap-1 text-[10px] text-emerald-400/80 font-mono"
                                title={`Holochain entry hash (tamper-proof anchor): ${entry.hash}`}
                              >
                                <LuShieldCheck class="w-3 h-3" />
                                {shortHash(entry.hash)}
                              </span>
                            </div>

                            {/* Content */}
                            <div
                              class="text-[var(--text-primary)] prose-sm pl-8"
                              dangerouslySetInnerHTML={renderMarkdown(entry.content)}
                            />

                            {entry.content && entry.content.trim() && (
                              <div class="pl-8 mt-1">
                                <RememberEntryButton aiId={aiId.value} text={entry.content} />
                              </div>
                            )}

                            <div class="pl-8">
                              {/* Attached context (user turns) */}
                              {entry.role === "user" && entry.attachments && (
                                <details class="mt-2">
                                  <summary class="text-xs text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)] flex items-center gap-1 select-none">
                                    <LuInfo class="w-3 h-3" />
                                    Attached context ({(entry.attachments.bytes / 1024).toFixed(0)} KB)
                                  </summary>
                                  <div class="mt-2 p-3 rounded-lg bg-[var(--bg-main)] text-xs space-y-2">
                                    <div class="text-[var(--text-muted)] break-all font-mono">sha256: {entry.attachments.sha256}</div>
                                    {entry.attachments.content ? (
                                      <pre class="p-2 rounded bg-[var(--bg-secondary)] text-[var(--text-secondary)] max-h-60 overflow-y-auto whitespace-pre-wrap text-[11px]">{entry.attachments.content}</pre>
                                    ) : (
                                      <div class="text-[var(--text-muted)]">Content too large to store inline — hash recorded for verification.</div>
                                    )}
                                  </div>
                                </details>
                              )}

                              {entry.role === "user" && entry.images && entry.images.length > 0 && (
                                <details class="mt-2">
                                  <summary class="text-xs text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)] flex items-center gap-1 select-none">
                                    <LuInfo class="w-3 h-3" />
                                    Attached image{entry.images.length > 1 ? `s (${entry.images.length})` : ""}
                                  </summary>
                                  <div class="mt-2 p-3 rounded-lg bg-[var(--bg-main)] text-xs space-y-3">
                                    {entry.images.map((img, ii) => (
                                      <div key={ii} class="space-y-1">
                                        {img.content ? (
                                          <img
                                            src={img.content}
                                            alt={img.filename}
                                            width={120}
                                            height={120}
                                            class="max-h-32 w-auto rounded-md border border-[var(--border-subtle)]"
                                          />
                                        ) : (
                                          <div class="text-[var(--text-muted)]">Image too large to store inline — hash recorded for verification.</div>
                                        )}
                                        <div class="text-[var(--text-muted)] break-all font-mono">
                                          {img.mime} · {(img.bytes / 1024).toFixed(0)} KB · sha256: {img.sha256}
                                        </div>
                                      </div>
                                    ))}
                                  </div>
                                </details>
                              )}

                              {/* Assistant: compact chip row + Provenance expand */}
                              {entry.role === "assistant" && (
                                <div class="mt-2">
                                  <div class="flex flex-wrap items-center gap-1.5 text-[10px]">
                                    <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-secondary)] font-mono">{entry.model}</span>
                                    {entry.runtime && (
                                      <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-muted)]">{entry.runtime.online ? "online" : "local"}</span>
                                    )}
                                    {entry.mode && (
                                      <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-muted)]">{entry.mode}</span>
                                    )}
                                    {entry.tokens && (
                                      <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-muted)]">
                                        {entry.tokens.tokens_per_second ? `${entry.tokens.tokens_per_second.toFixed(0)} tok/s · ` : ""}{entry.tokens.total_tokens} tok
                                      </span>
                                    )}
                                    {entry.sources && entry.sources.length > 0 && (
                                      <span class="px-1.5 py-0.5 rounded bg-[var(--bg-main)] text-[var(--text-link)]">{entry.sources.length} source{entry.sources.length !== 1 ? "s" : ""}</span>
                                    )}
                                  </div>

                                  {(entry.thinking || entry.system_prompt || (entry.sources && entry.sources.length > 0) || (entry.grounded && entry.grounded.length > 0) || entry.runtime) && (
                                    <details class="mt-2 group">
                                      <summary class="text-xs text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)] flex items-center gap-1 select-none">
                                        <LuInfo class="w-3 h-3" />
                                        Provenance
                                      </summary>
                                      <div class="mt-2 p-3 rounded-lg bg-[var(--bg-main)] text-xs space-y-3">
                                        {entry.runtime && (
                                          <div class="flex flex-wrap gap-x-3 gap-y-1 text-[var(--text-muted)]">
                                            <span>app {entry.runtime.app_version}</span>
                                            {entry.runtime.max_tokens != null && <span>max {entry.runtime.max_tokens} tok</span>}
                                            {entry.tokens && <span>{entry.tokens.completion_tokens} generated · {entry.tokens.total_tokens} total</span>}
                                          </div>
                                        )}
                                        {entry.system_prompt && (
                                          <details>
                                            <summary class="text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)] select-none">System prompt</summary>
                                            <pre class="mt-1 p-2 rounded bg-[var(--bg-secondary)] text-[var(--text-secondary)] max-h-60 overflow-y-auto whitespace-pre-wrap text-[11px]">{entry.system_prompt}</pre>
                                          </details>
                                        )}
                                        {entry.thinking && (
                                          <details>
                                            <summary class="text-[var(--text-muted)] cursor-pointer hover:text-[var(--text-secondary)] select-none">Thinking</summary>
                                            <div class="mt-1 p-2 rounded bg-[var(--bg-secondary)] text-[var(--text-secondary)] max-h-60 overflow-y-auto prose-sm" dangerouslySetInnerHTML={renderMarkdown(entry.thinking)} />
                                          </details>
                                        )}
                                        {entry.sources && entry.sources.length > 0 && (
                                          <div>
                                            <div class="text-[var(--text-muted)] mb-1">Sources</div>
                                            <ul class="space-y-0.5">
                                              {entry.sources.map((s, si) => (
                                                <li key={si}>
                                                  <a href={s.url} target="_blank" rel="noreferrer" class="text-[var(--text-link)] hover:underline break-all">{s.title || s.url}</a>
                                                </li>
                                              ))}
                                            </ul>
                                          </div>
                                        )}
                                        {entry.grounded && entry.grounded.length > 0 && (
                                          <div>
                                            <div class="text-[var(--text-muted)] mb-1">Grounded sources</div>
                                            <ul class="space-y-2">
                                              {entry.grounded.map((g, gi) => (
                                                <li key={gi}>
                                                  {g.kind === "document" ? (
                                                    <>
                                                      {g.claim && <div class="text-[var(--text-secondary)]">{g.claim}</div>}
                                                      {g.quote && (
                                                        <blockquote class="mt-0.5 pl-2 border-l-2 border-[var(--border-subtle)] text-[var(--text-muted)] italic break-words">“{g.quote}”</blockquote>
                                                      )}
                                                      <div class="mt-0.5 text-[10px] text-[var(--text-muted)] font-mono break-all">
                                                        {g.doc_name || "document"}
                                                        {g.span ? ` · chars ${g.span[0]}–${g.span[1]}` : " · quote located by content"}
                                                        {" · "}{g.doc_sha256.slice(0, 12)}…
                                                      </div>
                                                    </>
                                                  ) : (
                                                    <div class="text-[10px] text-[var(--text-muted)] font-mono break-all">🖼 {g.doc_name || "image"} · sha256 {g.doc_sha256.slice(0, 12)}…</div>
                                                  )}
                                                </li>
                                              ))}
                                            </ul>
                                          </div>
                                        )}
                                      </div>
                                    </details>
                                  )}
                                </div>
                              )}
                            </div>
                          </div>
                        ))
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>

      <ExportConversationModal
        isOpen={exportModalConv.value !== null}
        conversationTitle={exportModalConv.value?.title || "this conversation"}
        onClose$={$(() => (exportModalConv.value = null))}
        onExport$={$((opts, sign) => {
          const conv = exportModalConv.value;
          exportModalConv.value = null;
          if (conv) handleExport(conv, opts, sign);
        })}
      />

      <ConfirmModal
        isOpen={deleteModalConv.value !== null}
        title="Delete this conversation?"
        message="This removes the conversation and all its messages from your records. The deletion itself is signed into your chain - the record that something was deleted remains, the content does not. This can't be undone."
        confirmLabel="Delete conversation"
        cancelLabel="Cancel"
        variant="danger"
        busy={deletingConv.value}
        onConfirm$={$(async () => {
          const conv = deleteModalConv.value;
          if (!conv) return;
          deletingConv.value = true;
          deleteError.value = null;
          try {
            const { deleteConversation } = await import(
              "../../utils/holochainTranscripts"
            );
            await deleteConversation(conv.agent_key || agentKey.value, conv.hash);
            conversations.value = conversations.value.filter(
              (c) => c.hash !== conv.hash,
            );
            if (expandedHash.value === conv.hash) expandedHash.value = null;
          } catch (e) {
            console.warn("[Memory] delete conversation failed:", e);
            deleteError.value =
              "This conversation couldn't be deleted. Restart the app and try again - if it still fails, save a diagnostic report from Settings > Help & diagnostics.";
          } finally {
            deletingConv.value = false;
            deleteModalConv.value = null;
          }
        })}
        onCancel$={$(() => (deleteModalConv.value = null))}
      />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Memory - Your Own AI",
};
