/**
 * Your AIs Page - Unified AI management
 *
 * Shows all AIs (seeded defaults + user-created) in a single list.
 * All AIs are editable, deletable, and toggleable.
 */

import {
  component$,
  useSignal,
  useVisibleTask$,
  $,
} from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import { useAiData, useAiDataActions, canDeactivate } from "../../contexts/AiDataContext";
import type { UserDefinedAI } from "../../types";
import {
  LuPlusCircle,
  LuPencil,
  LuTrash2,
  LuPauseCircle,
  LuPlayCircle,
  LuBrain,
  LuArchive,
  LuRotateCcw,
} from "@qwikest/icons/lucide";
import { formatModelForCard } from "../../utils/modelNameFormatter";
import AppHeader from "../../components/AppHeader";
import AiFormModal from "../../components/AiFormModal";
import DeleteAiModal from "../../components/DeleteAiModal";
import ConfirmModal from "../../components/ConfirmModal";
import LastActiveAiModal from "../../components/LastActiveAiModal";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { Callout } from "../../components/Callout";

export default component$(() => {
  const nav = useNavigate();
  const aiData = useAiData();
  const {
    archiveCustomAi,
    restoreCustomAi,
    purgeCustomAi,
    updateCustomAiStatus,
    reorderAis,
  } = useAiDataActions();

  const deletingAiId = useSignal<string | null>(null);
  // Which action is running in the remove modal: 'archive' | 'purge' | null.
  const deleteBusyAction = useSignal<"archive" | "purge" | null>(null);

  // Drag-and-drop — all handlers registered natively on mount to bypass
  // Qwik QRL lazy-loading (which causes the first drag to be missed).
  const draggedAiId = useSignal<string | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    let currentDragId: string | null = null;

    const onDragStart = (e: DragEvent) => {
      const el = (e.target as HTMLElement).closest?.('[data-ai-id]') as HTMLElement | null;
      if (!el) return;
      currentDragId = el.dataset.aiId!;
      draggedAiId.value = currentDragId;
      if (e.dataTransfer) {
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', currentDragId);
      }
      setTimeout(() => { el.style.opacity = '0.4'; }, 0);
    };

    const onDragOver = (e: DragEvent) => {
      e.preventDefault();
      if (!currentDragId) return;

      const grid = (e.target as HTMLElement).closest('[data-ai-grid]') as HTMLElement | null;
      if (!grid) return;

      const cards = Array.from(grid.querySelectorAll('[data-ai-id]')) as HTMLElement[];
      if (cards.length < 2) return;

      let hoverIndex = -1;
      for (let i = 0; i < cards.length; i++) {
        const rect = cards[i].getBoundingClientRect();
        if (e.clientX >= rect.left && e.clientX <= rect.right && e.clientY >= rect.top && e.clientY <= rect.bottom) {
          hoverIndex = i;
          break;
        }
      }
      if (hoverIndex === -1) return;

      const draggedIdx = cards.findIndex(c => c.dataset.aiId === currentDragId);
      if (draggedIdx === -1 || draggedIdx === hoverIndex) return;

      // Live swap
      const ais = [...aiData.userDefinedAis];
      const [moved] = ais.splice(draggedIdx, 1);
      ais.splice(hoverIndex, 0, moved);
      aiData.userDefinedAis = ais;
    };

    const onDrop = (e: DragEvent) => {
      e.preventDefault();
      if (currentDragId) {
        reorderAis([...aiData.userDefinedAis]);
      }
      currentDragId = null;
      draggedAiId.value = null;
    };

    const onDragEnd = () => {
      const cards = document.querySelectorAll('[data-ai-id]');
      cards.forEach(c => ((c as HTMLElement).style.opacity = ''));
      currentDragId = null;
      draggedAiId.value = null;
    };

    const grid = document.querySelector('[data-ai-grid]');
    if (grid) {
      grid.addEventListener('dragstart', onDragStart as EventListener);
      grid.addEventListener('dragover', onDragOver as EventListener);
      grid.addEventListener('drop', onDrop as EventListener);
      grid.addEventListener('dragend', onDragEnd as EventListener);
    }

    cleanup(() => {
      if (grid) {
        grid.removeEventListener('dragstart', onDragStart as EventListener);
        grid.removeEventListener('dragover', onDragOver as EventListener);
        grid.removeEventListener('drop', onDrop as EventListener);
        grid.removeEventListener('dragend', onDragEnd as EventListener);
      }
    });
  });

  // Modal states
  const isAiModalOpen = useSignal(false);
  const isDeleteAiModalOpen = useSignal(false);
  const isLastActiveAiModalOpen = useSignal(false);
  const editingAi = useSignal<UserDefinedAI | null>(null);
  const aiToDelete = useSignal<{ id: string; name: string } | null>(null);
  const cardVersion = useSignal(0);
  const lastActiveAiName = useSignal("");
  const isLastAiDeleteModalOpen = useSignal(false);
  const lastAiDeleteName = useSignal("");

  // Model widget state
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  // Check if any default AIs are missing (for restore button)
  // Listen for model and settings changes
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    const handleStorageChange = async () => {
      const saved = localStorage.getItem("showModelWidget");
      showModelWidget.value = saved === "true";

      const storedModel = localStorage.getItem("currentModel");
      if (storedModel) {
        try {
          const models = await invoke<any[]>("list_local_models");
          if (!models || models.length === 0) {
            currentModel.value = null;
            localStorage.removeItem("currentModel");
          } else {
            const modelExists = models.some((m: any) => m.name === storedModel);
            currentModel.value = modelExists ? storedModel : null;
            if (!modelExists) localStorage.removeItem("currentModel");
          }
        } catch (error) {
          console.error("[YourAisPage] Error checking model:", error);
        }
      } else {
        currentModel.value = null;
      }
    };

    currentModel.value = localStorage.getItem("currentModel");
    const savedWidget = localStorage.getItem("showModelWidget");
    showModelWidget.value = savedWidget === "true";

    handleStorageChange();
    window.addEventListener("storage", handleStorageChange);
    window.addEventListener("focus", handleStorageChange);
    window.addEventListener("settingsChanged", handleStorageChange);
    window.addEventListener("modelDeleted", handleStorageChange);

    return () => {
      window.removeEventListener("storage", handleStorageChange);
      window.removeEventListener("focus", handleStorageChange);
      window.removeEventListener("settingsChanged", handleStorageChange);
      window.removeEventListener("modelDeleted", handleStorageChange);
    };
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });

  const handleModelsClick = $(() => {
    nav("/setup");
  });

  const handleDeleteAiClick = $((ai: UserDefinedAI) => {
    // Prevent removing the last AI that's still on the active list.
    const visible = aiData.userDefinedAis.filter((a) => a.status !== "archived");
    if (visible.length <= 1) {
      lastAiDeleteName.value = ai.name;
      isLastAiDeleteModalOpen.value = true;
      return;
    }
    aiToDelete.value = { id: ai.id, name: ai.name };
    isDeleteAiModalOpen.value = true;
  });

  const handleArchiveConfirm = $(async () => {
    if (!aiToDelete.value) return;
    deleteBusyAction.value = "archive";
    try {
      await archiveCustomAi(aiToDelete.value.id);
      isDeleteAiModalOpen.value = false;
      aiToDelete.value = null;
    } catch (error) {
      console.error("Failed to archive AI:", error);
      alert("Failed to archive AI: " + error);
    } finally {
      deleteBusyAction.value = null;
    }
  });

  const handlePurgeConfirm = $(async () => {
    if (!aiToDelete.value) return;
    deleteBusyAction.value = "purge";
    try {
      await purgeCustomAi(aiToDelete.value.id);
      isDeleteAiModalOpen.value = false;
      aiToDelete.value = null;
    } catch (error) {
      console.error("Failed to delete AI:", error);
      alert("Failed to delete AI: " + error);
    } finally {
      deleteBusyAction.value = null;
    }
  });

  const handleDeleteAiCancel = $(() => {
    if (deleteBusyAction.value) return; // don't dismiss mid-action
    isDeleteAiModalOpen.value = false;
    aiToDelete.value = null;
  });

  const handleRestoreAi = $(async (ai: UserDefinedAI) => {
    deletingAiId.value = ai.id;
    try {
      await restoreCustomAi(ai.id);
    } catch (error) {
      console.error("Failed to restore AI:", error);
      alert("Failed to restore AI: " + error);
    } finally {
      deletingAiId.value = null;
    }
  });

  // Archived-AI purge goes through a styled confirm (not the native dialog).
  const archivedToPurge = useSignal<UserDefinedAI | null>(null);

  const handlePurgeArchivedAi = $((ai: UserDefinedAI) => {
    archivedToPurge.value = ai;
  });

  const doPurgeArchivedAi = $(async () => {
    const ai = archivedToPurge.value;
    if (!ai) return;
    deletingAiId.value = ai.id;
    try {
      await purgeCustomAi(ai.id);
      archivedToPurge.value = null;
    } catch (error) {
      console.error("Failed to delete AI:", error);
      alert("Failed to delete AI: " + error);
    } finally {
      deletingAiId.value = null;
    }
  });

  const handleToggleAiStatus = $(async (ai: UserDefinedAI) => {
    const newStatus = ai.status === "active" ? "inactive" : "active";

    if (newStatus === "inactive" && !canDeactivate(aiData, ai.id)) {
      lastActiveAiName.value = ai.name;
      isLastActiveAiModalOpen.value = true;
      return;
    }

    try {
      await updateCustomAiStatus(ai.id, newStatus);
    } catch (error) {
      console.error("Failed to update AI status:", error);
      alert("Failed to update AI status: " + error);
    }
  });

  const handleCreateNewAi = $(() => {
    editingAi.value = null;
    isAiModalOpen.value = true;
  });

  const handleEditAi = $((ai: UserDefinedAI) => {
    editingAi.value = ai;
    isAiModalOpen.value = true;
  });


  /**
   * Get thumbnail URL for an AI, checking (in order):
   * 1. Custom uploaded thumbnail
   * 2. Static default thumb (veebo/teresa/reeves)
   * 3. Archetype default thumbnail
   * 4. Generic placeholder
   */
  const getThumbnail = (ai: UserDefinedAI): string => {
    const custom = aiData.thumbnailObjectUrls[ai.id];
    if (custom && custom !== "error") return custom;
    const archetypeThumb = aiData.archetypeDefaultThumbnailsMap[ai.baseArchetypeId];
    if (archetypeThumb) return archetypeThumb;
    return "/generic-ai-placeholder.svg";
  };

  // The personality name (Wizard, Caregiver, ...) — the archetype `category`
  // field is "General" for nearly every personality, so it read as a stuck
  // label that never followed personality changes.
  const getPersonalityName = (ai: UserDefinedAI): string => {
    const archetype = aiData.archetypeTemplates.find((a) => a.id === ai.baseArchetypeId);
    return archetype?.name || "Custom";
  };

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      {/* Header */}
      <div class="relative z-10">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
      </div>

      {/* Content */}
      <div class="flex-1 overflow-y-auto p-6">
        <div class="max-w-7xl mx-auto">
          {/* Section Header */}
          <div class="flex justify-between items-baseline mb-8">
            <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela">
              Your AIs
            </h2>
            <div class="flex items-center gap-2">
              <LiquidMetalButton
                onClick$={handleCreateNewAi}
                class="flex items-center h-9 px-4 sm:px-5 text-[0.9375rem] transition-all duration-150 ease-in-out"
              >
                <LuPlusCircle class="w-5 h-5 mr-1" />
                Create New AI
              </LiquidMetalButton>
            </div>
          </div>

          {/* AIs Grid */}
          {aiData.isLoadingUserAis ? (
            <div class="text-center py-12">
              <div class="inline-block w-8 h-8 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
              <p class="mt-4 text-[var(--text-secondary)]">Loading your AIs...</p>
            </div>
          ) : aiData.userDefinedAis.filter((ai) => ai.status !== "archived").length === 0 ? (
            <div class="generic-container p-8 rounded-2xl text-center">
              <p class="text-[var(--text-secondary)] mb-4">
                No active AIs. Click "Create New AI", restore one from Archived below, or "Restore Defaults" to get started.
              </p>
            </div>
          ) : (
            <div
              data-ai-grid
              class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6"
            >
              {aiData.userDefinedAis.filter((ai) => ai.status !== "archived").map((ai) => (
                  <div
                    key={`${ai.id}-${cardVersion.value}`}
                    data-ai-id={ai.id}
                    draggable={true}
                    class={`generic-container rounded-2xl overflow-hidden flex flex-col justify-between transition-all hover:shadow-2xl transform hover:-translate-y-1 ${
                      ai.status === "inactive" ? "opacity-50" : ""
                    } ${draggedAiId.value === ai.id ? "opacity-40 scale-95" : ""}`}
                    style={{ cursor: "grab" }}
                  >
                  <div class="p-5 relative">
                    {/* Drag handle */}
                    <div class="absolute top-3 right-3 text-[var(--text-muted)] cursor-grab opacity-40 hover:opacity-100 transition-opacity" title="Drag to reorder">
                      <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                        <circle cx="5" cy="3" r="1.5" />
                        <circle cx="11" cy="3" r="1.5" />
                        <circle cx="5" cy="8" r="1.5" />
                        <circle cx="11" cy="8" r="1.5" />
                        <circle cx="5" cy="13" r="1.5" />
                        <circle cx="11" cy="13" r="1.5" />
                      </svg>
                    </div>
                    <div class="flex items-start mb-4">
                      <div class="relative mr-4 flex-shrink-0">
                        <div class="w-16 h-16 rounded-full border-2 border-[var(--border-subtle)] bg-[var(--bg-dropdown)] flex items-center justify-center overflow-hidden">
                          <img
                            src={getThumbnail(ai)}
                            alt={`${ai.name} thumbnail`}
                            class="w-full h-full object-cover object-center"
                            onError$={(e: Event) => {
                              const img = e.currentTarget as HTMLImageElement;
                              if (!img.src.endsWith("/generic-ai-placeholder.svg")) {
                                img.src = "/generic-ai-placeholder.svg";
                              }
                            }}
                          />
                        </div>
                      </div>
                      <div class="overflow-hidden flex-1">
                        <h3
                          class="text-lg font-semibold text-[var(--text-primary)] truncate"
                          title={ai.name}
                        >
                          {ai.name}
                        </h3>
                        <p class="text-xs text-[var(--text-secondary)] truncate">
                          {getPersonalityName(ai)} • {ai.status === "active" ? "Active" : "Inactive"}
                        </p>
                        <p class="text-xs text-[var(--text-muted)] truncate" title={ai.model}>
                          {formatModelForCard(ai.model)}
                        </p>
                      </div>
                    </div>
                    <p class="text-sm text-[var(--text-secondary)] line-clamp-3 overflow-hidden">
                      {ai.description || "No description"}
                    </p>
                  </div>
                  <div class="px-5 py-3 mt-auto">
                    <div class="flex justify-between items-center">
                      <LiquidMetalButton
                        onClick$={() => {
                          sessionStorage.setItem("memory-ai-id", ai.id);
                          sessionStorage.setItem("memory-agent-key", ai.agentPubKey || "");
                          nav("/memory/");
                        }}
                        title="Memory"
                        class="px-3 py-1.5 transition-colors duration-150 flex items-center gap-1.5 rounded-full text-xs"
                      >
                        <LuBrain class="w-[14px] h-[14px]" />
                        <span>Memory</span>
                      </LiquidMetalButton>
                      <div class="flex gap-2">
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={() => handleEditAi(ai)}
                        title="Edit AI"
                        class="p-2"
                      >
                        <LuPencil class="w-[18px] h-[18px]" />
                      </LiquidMetalButton>
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={() => handleToggleAiStatus(ai)}
                        title={ai.status === "active" ? "Deactivate" : "Activate"}
                        class="p-2"
                      >
                        {ai.status === "active" ? (
                          <LuPauseCircle class="w-[18px] h-[18px]" />
                        ) : (
                          <LuPlayCircle class="w-[18px] h-[18px]" />
                        )}
                      </LiquidMetalButton>
                      <LiquidMetalButton
                        variant="danger"
                        onClick$={() => handleDeleteAiClick(ai)}
                        disabled={deletingAiId.value === ai.id}
                        title="Delete AI"
                        class="p-2"
                      >
                        {deletingAiId.value === ai.id ? (
                          <div class="w-[18px] h-[18px] border-2 border-current border-t-transparent rounded-full animate-spin" />
                        ) : (
                          <LuTrash2 class="w-[18px] h-[18px]" />
                        )}
                      </LiquidMetalButton>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Archived AIs — retired but recoverable */}
          {aiData.userDefinedAis.filter((ai) => ai.status === "archived").length > 0 && (
            <div class="mt-10">
              <h3 class="text-sm font-semibold text-[var(--text-secondary)] mb-1 flex items-center gap-2">
                <LuArchive class="w-4 h-4" />
                Archived
              </h3>
              <p class="text-xs text-[var(--text-muted)] mb-4">
                Hidden from your list. Their conversations and memory are kept — restore anytime, or delete for good.
              </p>
              <div class="space-y-2">
                {aiData.userDefinedAis
                  .filter((ai) => ai.status === "archived")
                  .map((ai) => (
                    <div
                      key={ai.id}
                      class="flex items-center gap-3 p-3 rounded-xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] opacity-90"
                    >
                      <div class="w-10 h-10 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-card)] flex items-center justify-center overflow-hidden flex-shrink-0">
                        <img
                          src={getThumbnail(ai)}
                          alt={`${ai.name} thumbnail`}
                          class="w-full h-full object-cover object-center grayscale"
                          onError$={(e: Event) => {
                            const img = e.currentTarget as HTMLImageElement;
                            if (!img.src.endsWith("/generic-ai-placeholder.svg")) {
                              img.src = "/generic-ai-placeholder.svg";
                            }
                          }}
                        />
                      </div>
                      <div class="min-w-0 flex-1">
                        <p class="text-sm font-medium text-[var(--text-primary)] truncate">{ai.name}</p>
                        <p class="text-xs text-[var(--text-muted)] truncate">{getPersonalityName(ai)} • Archived</p>
                      </div>
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={() => handleRestoreAi(ai)}
                        disabled={deletingAiId.value === ai.id}
                        class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                      >
                        {deletingAiId.value === ai.id ? (
                          <div class="w-3.5 h-3.5 border-2 border-current border-t-transparent rounded-full animate-spin" />
                        ) : (
                          <LuRotateCcw class="w-3.5 h-3.5" />
                        )}
                        Restore
                      </LiquidMetalButton>
                      <LiquidMetalButton
                        variant="danger"
                        onClick$={() => handlePurgeArchivedAi(ai)}
                        disabled={deletingAiId.value === ai.id}
                        title="Delete permanently"
                        class="p-2"
                      >
                        <LuTrash2 class="w-4 h-4" />
                      </LiquidMetalButton>
                    </div>
                  ))}
              </div>
            </div>
          )}

          {/* Help tip — what an AI is + that they're yours to shape */}
          <Callout intent="premium" title="Make it your own" id="your-ais-intro" class="mt-8">
            Each AI is a personality with its own model and its own private,
            encrypted memory. Edit or delete the ones that come built in, and make
            as many of your own as you like — a coder, a writer, a brainstorm
            partner. Switch any of them between offline and online models anytime.
          </Callout>
        </div>
      </div>

      {/* Modals */}
      {isAiModalOpen.value && (
        <AiFormModal
          isOpen={isAiModalOpen.value}
          onClose$={$(() => {
            isAiModalOpen.value = false;
            cardVersion.value++;
          })}
          editingAi={editingAi.value}
          currentDisplayableThumbnailUrl={
            editingAi.value
              ? (aiData.thumbnailObjectUrls[editingAi.value.id] &&
                 aiData.thumbnailObjectUrls[editingAi.value.id] !== "error"
                  ? aiData.thumbnailObjectUrls[editingAi.value.id]
                  : null)
              : null
          }
        />
      )}

      <DeleteAiModal
        isOpen={isDeleteAiModalOpen.value}
        onClose$={handleDeleteAiCancel}
        onArchive$={handleArchiveConfirm}
        onPurge$={handlePurgeConfirm}
        aiName={aiToDelete.value?.name || ""}
        busyAction={deleteBusyAction.value}
      />

      <ConfirmModal
        isOpen={!!archivedToPurge.value}
        variant="danger"
        title={`Permanently delete ${archivedToPurge.value?.name ?? "this AI"}?`}
        message="This erases its signed conversations and everything it learned about you. It can't be undone."
        confirmLabel="Delete permanently"
        busy={!!archivedToPurge.value && deletingAiId.value === archivedToPurge.value?.id}
        onConfirm$={doPurgeArchivedAi}
        onCancel$={() => (archivedToPurge.value = null)}
      />

      <LastActiveAiModal
        isOpen={isLastActiveAiModalOpen.value}
        onClose={$(() => {
          isLastActiveAiModalOpen.value = false;
        })}
        aiName={lastActiveAiName.value}
      />

      <LastActiveAiModal
        isOpen={isLastAiDeleteModalOpen.value}
        onClose={$(() => {
          isLastAiDeleteModalOpen.value = false;
        })}
        aiName={lastAiDeleteName.value}
        title="Cannot Delete AI"
        message="You must have at least one AI."
        detail="Create another AI before deleting this one."
      />
    </div>
  );
});

export const head: DocumentHead = {
  title: "Your AIs - Your Own AI",
};
