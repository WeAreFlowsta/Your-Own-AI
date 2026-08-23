/**
 * Model Manager API
 * 
 * Handles downloading and managing local GGUF models from Hugging Face.
 * Works with the Rust llm module for model storage and management.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { LocalModel } from '../types';
import { EMBEDDING_MODEL, VISION_PROJECTORS, OCR_MODELS } from '../data/recommended-models';

/** Filenames that are capability dependencies (embedding, vision projectors), not
 *  chat models — they live in the same models dir but must never appear in a model
 *  picker. Matched by exact filename, plus a defensive `mmproj` guard for any
 *  projector not in the catalog. */
const NON_CHAT_MODEL_FILES = new Set<string>([
  EMBEDDING_MODEL.filename,
  ...VISION_PROJECTORS.map((p) => p.filename),
  ...(OCR_MODELS.files?.map((f) => f.filename) ?? [OCR_MODELS.filename]),
]);
const isNonChatModel = (name: string): boolean =>
  NON_CHAT_MODEL_FILES.has(name) || /mmproj/i.test(name) || /\.rten$/i.test(name);

export interface DownloadProgress {
  filename: string;
  downloaded: number;
  total: number;
  percent: number;
}

export interface DownloadComplete {
  filename: string;
  path: string;
}

export class ModelManager {
  /**
   * List all downloaded models
   */
  async listModels(): Promise<LocalModel[]> {
    // Usable chat models only: no capability dependencies (embedding, vision
    // projectors) and no damaged files - those are inventory, not choices.
    return (await this.listAllModels()).filter((m) => !m.damaged);
  }

  /**
   * Every model file on disk, including damaged ones (incomplete or
   * corrupted downloads) - for the inventory, where a damaged file must be
   * visible so it can be deleted and fetched again.
   */
  async listAllModels(): Promise<LocalModel[]> {
    try {
      const all = await invoke<LocalModel[]>('list_local_models');
      // Hide capability dependencies (embedding, vision projectors) — they're in
      // the models dir but are not selectable chat models.
      return all.filter((m) => !isNonChatModel(m.name));
    } catch (error) {
      console.error('[ModelManager] Error listing models:', error);
      return [];
    }
  }

  /**
   * Check if a specific model is downloaded
   */
  async isModelDownloaded(filename: string): Promise<boolean> {
    try {
      return await invoke<boolean>('is_model_downloaded', { filename });
    } catch (error) {
      console.error('[ModelManager] Error checking model:', error);
      return false;
    }
  }

  /**
   * Whether a download for this file is in progress (so the UI can reattach) and
   * whether a resumable partial exists. Lets a card pick a download back up after
   * the user navigated away.
   */
  async downloadStatus(
    filename: string,
  ): Promise<{ downloading: boolean; has_partial: boolean; downloaded_bytes: number; total_bytes: number }> {
    try {
      return await invoke('download_status', { filename });
    } catch {
      return { downloading: false, has_partial: false, downloaded_bytes: 0, total_bytes: 0 };
    }
  }

  /**
   * Get the models directory path
   */
  async getModelsDirectory(): Promise<string> {
    try {
      return await invoke<string>('get_models_directory');
    } catch (error) {
      console.error('[ModelManager] Error getting models directory:', error);
      throw error;
    }
  }

  /**
   * Download a model from Hugging Face with progress tracking
   */
  async downloadModel(
    url: string,
    filename: string,
    onProgress?: (progress: DownloadProgress) => void
  ): Promise<void> {
    let unlistenProgress: UnlistenFn | null = null;
    let unlistenComplete: UnlistenFn | null = null;

    try {
      // Listen for progress events (filter by filename to prevent cross-download contamination)
      if (onProgress) {
        unlistenProgress = await listen<DownloadProgress>(
          'model-download-progress',
          (event) => {
            // Only process events for this specific download
            if (event.payload.filename === filename) {
              onProgress(event.payload);
            }
          }
        );
      }

      // Listen for completion event (filter by filename)
      const completionPromise = new Promise<void>((resolve, reject) => {
        listen<DownloadComplete>('model-download-complete', (event) => {
          // Only process completion for this specific download
          if (event.payload.filename === filename) {
            console.log('[ModelManager] Download complete:', event.payload);
            resolve();
          }
        }).then(unlisten => {
          unlistenComplete = unlisten;
        });

        // Start download
        invoke('download_model', { url, filename })
          .then(() => {
            // Wait a bit for the completion event
            setTimeout(resolve, 100);
          })
          .catch(reject);
      });

      await completionPromise;
      
      console.log('[ModelManager] Model downloaded successfully:', filename);
    } catch (error) {
      console.error('[ModelManager] Download failed:', error);
      throw error;
    } finally {
      // Cleanup listeners
      try {
        if (unlistenProgress) unlistenProgress();
        // @ts-ignore - TypeScript incorrectly infers this as never
        if (unlistenComplete) unlistenComplete();
      } catch (e) {
        // Ignore errors during cleanup
      }
    }
  }

  /**
   * Delete a downloaded model
   */
  async deleteModel(filename: string): Promise<void> {
    try {
      await invoke('delete_model', { filename });
      console.log('[ModelManager] Model deleted:', filename);
    } catch (error) {
      console.error('[ModelManager] Error deleting model:', error);
      throw error;
    }
  }

  /**
   * Get model file size in GB
   */
  getModelSizeGB(model: LocalModel): number {
    return model.size_bytes / (1024 ** 3);
  }

  /**
   * Format model size for display
   */
  formatModelSize(sizeBytes: number): string {
    const gb = sizeBytes / (1024 ** 3);
    if (gb < 1) {
      const mb = sizeBytes / (1024 ** 2);
      return `${mb.toFixed(0)}MB`;
    }
    return `${gb.toFixed(1)}GB`;
  }
}

// Export singleton instance
export const modelManager = new ModelManager();
