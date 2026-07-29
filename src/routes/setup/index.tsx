/**
 * Setup Page - Offline Models Setup
 *
 * Guides users through downloading their first offline AI model.
 * Shows recommendations based on system RAM.
 */

import {
  component$,
  useSignal,
  useVisibleTask$,
  $,
} from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { ModelDownloader } from "../../components/ModelDownloader";
import AppHeader from "../../components/AppHeader";
import { invoke } from "@tauri-apps/api/core";

export interface SystemInfo {
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
  const systemInfo = useSignal<SystemInfo | null>(null);
  const loading = useSignal(true);

  // Model widget state
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  // Listen for model and settings changes
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    // Init from localStorage
    currentModel.value = localStorage.getItem("currentModel");
    const savedWidget = localStorage.getItem("showModelWidget");
    showModelWidget.value = savedWidget === "true";

    const handleStorageChange = async () => {
      // Check settings
      const saved = localStorage.getItem("showModelWidget");
      showModelWidget.value = saved === "true";

      // Check if current model still exists
      const storedModel = localStorage.getItem("currentModel");
      if (storedModel) {
        try {
          const models = await invoke<any[]>("list_local_models");
          if (!models || models.length === 0) {
            console.log("[SetupPage] No models found, clearing currentModel");
            currentModel.value = null;
            localStorage.removeItem("currentModel");
          } else {
            const modelExists = models.some(
              (m: any) => m.name === storedModel
            );
            if (modelExists) {
              currentModel.value = storedModel;
            } else {
              console.log(
                "[SetupPage] Stored model no longer exists, clearing"
              );
              currentModel.value = null;
              localStorage.removeItem("currentModel");
            }
          }
        } catch (error) {
          console.error(
            "[SetupPage] Error checking model existence:",
            error
          );
        }
      } else {
        currentModel.value = null;
      }
    };

    handleStorageChange(); // Check on mount
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

  // Load system info on mount
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const info = await invoke<SystemInfo>("get_system_info");
      systemInfo.value = info;
    } catch (error) {
      console.error("Failed to load system info:", error);
    } finally {
      loading.value = false;
    }
  });

  const handleModelSelected = $((filename: string) => {
    console.log("[Setup] Model selected:", filename);
    invoke("load_model", { filename, withVision: false, reason: "setup" })
      .then(() => {
        console.log("[Setup] Model loaded, navigating to chat");
        nav("/chat");
      })
      .catch((error) => {
        console.error("[Setup] Failed to load model:", error);
        alert("Failed to load model: " + error);
      });
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });

  const handleModelsClick = $(() => {
    // Already on setup page, no-op
  });

  if (loading.value) {
    return (
      <div class="flex flex-col h-screen bg-[var(--bg-main)]">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
        <div class="flex-1 flex items-center justify-center">
          <div class="text-center">
            <div class="w-16 h-16 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin mx-auto mb-4" />
            <p class="text-[var(--text-secondary)]">
              Loading system information...
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      {/* Header */}
      {/* z-20: above page content so header dropdowns (projects, menus)
          are never painted over by in-page stacking contexts like the
          AI selector row (z-10). Page modals are fixed z-50 and still
          cover the header. */}
      <div class="relative z-20">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
      </div>

      {/* Content */}
      <div class="flex-1 overflow-y-auto">
        <div class="py-6">
          {/* Offline Models Downloader */}
          <ModelDownloader
            systemInfo={systemInfo.value}
            onModelSelected$={handleModelSelected}
          />
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Setup - Your Own AI",
};
