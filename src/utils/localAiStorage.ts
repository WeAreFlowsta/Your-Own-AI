/**
 * Local AI Storage System
 * 
 * Manages user-created custom AIs in local-only mode using Tauri Store.
 * When user switches to cloud mode, these can be synced to Firestore.
 */

import { UserDefinedAI, LengthDisposition } from '../types';
import { Store } from '@tauri-apps/plugin-store';
import { getDefaultPersonalities, getArchetypeById } from '../data/bundled-archetypes';
import { MISSION_CORE } from '../data/missionCore';

/**
 * Migrate the old `responseLengthId` (chat/normal/report) → the new
 * `lengthDisposition`. Idempotent: passes through already-migrated values.
 */
export function toDisposition(old: string | undefined | null): LengthDisposition {
  switch (old) {
    case 'thorough':
    case 'balanced':
    case 'conversational':
      return old;
    case 'report':
      return 'thorough';
    case 'normal':
      return 'balanced';
    case 'chat':
    default:
      return 'conversational';
  }
}

/** Ensure an AI loaded from the store uses the new fields (migrate the legacy
 *  `responseLengthId`). Legacy "report" was an always-report MODE, not a length —
 *  so it becomes a neutral length + `defaultMode: 'report'`. */
export function migrateAiDisposition(ai: UserDefinedAI): UserDefinedAI {
  if (ai.lengthDisposition && ai.defaultMode) return ai;
  const legacy = (ai as unknown as { responseLengthId?: string }).responseLengthId;
  const patched = { ...ai };
  if (!patched.lengthDisposition) {
    patched.lengthDisposition = legacy === 'report' ? 'balanced' : toDisposition(legacy);
  }
  if (!patched.defaultMode) {
    patched.defaultMode = legacy === 'report' ? 'report' : 'chat';
  }
  return patched;
}

const STORE_PATH = 'ai-data.json';
const CUSTOM_AIS_KEY = 'custom-ais';
const SYNC_STATUS_KEY = 'sync-status';
const DEFAULTS_SEEDED_KEY = 'defaults-seeded';

/** The archetype IDs of the three default AIs that ship with the app */
export const DEFAULT_ARCHETYPE_IDS = ['veebo', 'teresa', 'reeves'] as const;

let storeInstance: Store | null = null;

/**
 * Get or initialize the store instance
 */
async function getStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = await Store.load(STORE_PATH);
  }
  return storeInstance;
}

/**
 * Seed default AIs (Veebo, Teresa, Reeves) into the store on first launch.
 * Migrates any existing preferences from localStorage.
 * Idempotent — skips if already seeded or if the AIs already exist.
 */
export async function seedDefaultAisIfNeeded(): Promise<void> {
  try {
    const store = await getStore();

    // Check if already seeded
    const alreadySeeded = await store.get<boolean>(DEFAULTS_SEEDED_KEY);
    if (!alreadySeeded) {

    const existingAis = await store.get<UserDefinedAI[]>(CUSTOM_AIS_KEY) || [];

    // Read any existing preferences from localStorage (migration)
    let oldPrefs: Record<string, { model?: string; responseLengthId?: string; useEmojis?: boolean; status?: string }> = {};
    try {
      const stored = localStorage.getItem('defaultAiPreferences');
      if (stored) {
        oldPrefs = JSON.parse(stored);
      }
    } catch {
      // Ignore parse errors
    }

    // Default AI display names and descriptions (archetypes provide the system prompts)
    const defaultAiNames: Record<string, { name: string; description: string }> = {
      veebo: { name: 'Veebo', description: 'A witty, truth-seeking AI robot.' },
      teresa: { name: 'Teresa', description: 'A nurturing, empathetic, and empowering AI.' },
      reeves: { name: 'Reeves', description: 'A helpful, insightful AI with a neutral personality.' },
    };

    const defaults = getDefaultPersonalities();
    const newAis: UserDefinedAI[] = [];

    for (const archetype of defaults) {
      // Check if we already have an AI based on this archetype
      const alreadyHas = existingAis.some(ai => ai.baseArchetypeId === archetype.id);
      if (alreadyHas) continue;

      const prefs = oldPrefs[archetype.id] || {};
      const display = defaultAiNames[archetype.id] || { name: archetype.name, description: archetype.description };

      const ai: UserDefinedAI = {
        id: `local-${Date.now()}-${Math.random().toString(36).slice(2, 11)}`,
        name: display.name,
        description: display.description,
        baseArchetypeId: archetype.id,
        // Default to Auto (offline) so every AI always has a valid model — the
        // router picks the best downloaded model per query, no manual selection.
        model: prefs.model || 'auto:offline',
        systemPrompt: archetype.systemPromptTemplate,
        status: (prefs.status as 'active' | 'inactive') || 'active',
        lengthDisposition: toDisposition(prefs.responseLengthId),
        defaultMode: 'chat',
        useEmojis: prefs.useEmojis ?? false,
      };

      newAis.push(ai);
    }

    if (newAis.length > 0) {
      const allAis = [...newAis, ...existingAis];
      await store.set(CUSTOM_AIS_KEY, allAis);
      console.log(`[LocalAiStorage] Seeded ${newAis.length} default AIs`);
    }

      await store.set(DEFAULTS_SEEDED_KEY, true);
      await store.save();

      // Clean up old localStorage preferences
      localStorage.removeItem('defaultAiPreferences');
    }

    // Ensure default AI thumbnails exist on disk as custom thumbnails
    const { invoke } = await import('@tauri-apps/api/core');
    const defaultThumbSources: Record<string, string> = {
      veebo: '/bundled/veebo.jpg',
      teresa: '/bundled/teresa.jpg',
      reeves: '/bundled/reeves.jpg',
    };
    const allAis = await store.get<UserDefinedAI[]>(CUSTOM_AIS_KEY) || [];
    for (const ai of allAis) {
      const thumbSource = defaultThumbSources[ai.baseArchetypeId];
      if (!thumbSource) continue;
      // Check if custom thumbnail already exists on disk
      try {
        await invoke<number[]>('get_ai_thumbnail', { aiId: ai.id });
        // Already exists, skip
      } catch {
        // No custom thumbnail on disk — save the default character face
        try {
          const response = await fetch(thumbSource);
          const blob = await response.blob();
          const buffer = await blob.arrayBuffer();
          await invoke('save_ai_thumbnail', {
            aiId: ai.id,
            thumbnailData: Array.from(new Uint8Array(buffer)),
          });
          console.log(`[LocalAiStorage] Saved default thumbnail for ${ai.name}`);
        } catch (err) {
          console.warn(`[LocalAiStorage] Failed to save thumbnail for ${ai.name}:`, err);
        }
      }
    }
  } catch (error) {
    console.error('[LocalAiStorage] Error seeding default AIs:', error);
  }
}

/**
 * Re-seed any missing default AIs (e.g. after user deleted them and wants them back)
 */
export async function reseedMissingDefaults(): Promise<UserDefinedAI[]> {
  const store = await getStore();
  const existingAis = await store.get<UserDefinedAI[]>(CUSTOM_AIS_KEY) || [];

  const defaultAiNames: Record<string, { name: string; description: string }> = {
    veebo: { name: 'Veebo', description: 'A witty, truth-seeking AI robot.' },
    teresa: { name: 'Teresa', description: 'A nurturing, empathetic, and empowering AI.' },
    reeves: { name: 'Reeves', description: 'A helpful, insightful AI with a neutral personality.' },
  };

  const defaults = getDefaultPersonalities();
  const restored: UserDefinedAI[] = [];

  for (const archetype of defaults) {
    // Check if we already have an AI based on this archetype
    const alreadyHas = existingAis.some(ai => ai.baseArchetypeId === archetype.id);
    if (alreadyHas) continue;

    const display = defaultAiNames[archetype.id] || { name: archetype.name, description: archetype.description };

    const ai: UserDefinedAI = {
      id: `local-${Date.now()}-${Math.random().toString(36).slice(2, 11)}`,
      name: display.name,
      description: display.description,
      baseArchetypeId: archetype.id,
      model: 'auto:offline',
      systemPrompt: archetype.systemPromptTemplate,
      status: 'active',
      lengthDisposition: 'conversational',
      defaultMode: 'chat',
      useEmojis: false,
    };

    restored.push(ai);
  }

  if (restored.length > 0) {
    const allAis = [...restored, ...existingAis];
    await store.set(CUSTOM_AIS_KEY, allAis);
    await store.save();
    console.log(`[LocalAiStorage] Restored ${restored.length} default AIs`);
  }

  return restored;
}

/**
 * Get all locally stored custom AIs
 */
/**
 * Materialize each AI's stored systemPrompt as the CURRENT mission-core + its
 * archetype's live persona template. The in-app chat resolves the template live,
 * but the inference API (Rust) reads this stored prompt — so republishing it here
 * keeps the API's persona identical to in-app, from one source (the templates).
 * Only writes when something actually changed (a persona update or new template).
 */
export async function republishStoredPersonas(): Promise<void> {
  try {
    const store = await getStore();
    const ais = (await store.get<UserDefinedAI[]>(CUSTOM_AIS_KEY)) || [];
    if (ais.length === 0) return;
    let changed = false;
    const next = ais.map((ai) => {
      const archetype = getArchetypeById(ai.baseArchetypeId);
      const freshPrompt = archetype
        ? `${MISSION_CORE}\n\n${archetype.systemPromptTemplate}`
        : ai.systemPrompt;
      // Migrate AIs with no model to Auto (offline). An empty model breaks the
      // inference API ("no model configured") and in-app silently rode the
      // globally-loaded model; Auto is always valid and router-picked.
      const model = ai.model && ai.model.trim() ? ai.model : 'auto:offline';
      if (ai.systemPrompt === freshPrompt && ai.model === model) return ai;
      changed = true;
      return { ...ai, systemPrompt: freshPrompt, model };
    });
    if (changed) {
      await store.set(CUSTOM_AIS_KEY, next);
      await store.save();
      console.log('[LocalAiStorage] Republished stored personas to current templates (for the inference API)');
    }
  } catch (e) {
    console.error('[LocalAiStorage] republishStoredPersonas failed:', e);
  }
}

export async function getLocalCustomAis(): Promise<UserDefinedAI[]> {
  try {
    const store = await getStore();
    const ais = await store.get<UserDefinedAI[]>(CUSTOM_AIS_KEY);
    
    if (!ais) {
      console.log('[LocalAiStorage] No custom AIs found, returning empty array');
      return [];
    }
    
    console.log(`[LocalAiStorage] Retrieved ${ais.length} custom AIs`);
    // Migrate any legacy responseLengthId → lengthDisposition on read.
    return ais.map(migrateAiDisposition);
  } catch (error) {
    console.error('[LocalAiStorage] Error getting custom AIs:', error);
    return [];
  }
}

/**
 * Save a new custom AI locally
 */
export async function saveLocalCustomAi(ai: Omit<UserDefinedAI, 'id' | 'status'>): Promise<UserDefinedAI> {
  try {
    const store = await getStore();
    const existingAis = await getLocalCustomAis();
    
    // Generate a unique ID with "local-" prefix
    const newAi: UserDefinedAI = {
      ...ai,
      id: `local-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      status: 'active'
    };
    
    const updatedAis = [...existingAis, newAi];
    await store.set(CUSTOM_AIS_KEY, updatedAis);
    await store.save();
    
    console.log('[LocalAiStorage] Saved new custom AI:', newAi.id, newAi.name);
    return newAi;
  } catch (error) {
    console.error('[LocalAiStorage] Error saving custom AI:', error);
    throw new Error('Failed to save custom AI locally');
  }
}

/**
 * Update an existing custom AI locally
 */
export async function updateLocalCustomAi(
  id: string, 
  updates: Partial<UserDefinedAI>
): Promise<UserDefinedAI | null> {
  try {
    const store = await getStore();
    const existingAis = await getLocalCustomAis();
    
    const aiIndex = existingAis.findIndex(ai => ai.id === id);
    if (aiIndex === -1) {
      console.warn('[LocalAiStorage] AI not found for update:', id);
      return null;
    }
    
    const updatedAi: UserDefinedAI = {
      ...existingAis[aiIndex],
      ...updates,
    };
    
    existingAis[aiIndex] = updatedAi;
    await store.set(CUSTOM_AIS_KEY, existingAis);
    await store.save();
    
    console.log('[LocalAiStorage] Updated custom AI:', id, updatedAi.name);
    return updatedAi;
  } catch (error) {
    console.error('[LocalAiStorage] Error updating custom AI:', error);
    throw new Error('Failed to update custom AI locally');
  }
}

/**
 * Delete a custom AI locally
 */
export async function deleteLocalCustomAi(id: string): Promise<boolean> {
  try {
    const store = await getStore();
    const existingAis = await getLocalCustomAis();
    
    const filteredAis = existingAis.filter(ai => ai.id !== id);
    
    if (filteredAis.length === existingAis.length) {
      console.warn('[LocalAiStorage] AI not found for deletion:', id);
      return false;
    }
    
    await store.set(CUSTOM_AIS_KEY, filteredAis);
    await store.save();
    
    console.log('[LocalAiStorage] Deleted custom AI:', id);
    return true;
  } catch (error) {
    console.error('[LocalAiStorage] Error deleting custom AI:', error);
    throw new Error('Failed to delete custom AI locally');
  }
}

/**
 * Update custom AI status (active/inactive)
 */
export async function updateLocalCustomAiStatus(
  id: string, 
  status: 'active' | 'inactive'
): Promise<UserDefinedAI | null> {
  return updateLocalCustomAi(id, { status });
}

/**
 * Get sync status information
 */
export async function getSyncStatus(): Promise<{ lastSyncTime: string; userId: string } | null> {
  try {
    const store = await getStore();
    const status = await store.get<{ lastSyncTime: string; userId: string }>(SYNC_STATUS_KEY);
    return status || null;
  } catch (error) {
    console.error('[LocalAiStorage] Error getting sync status:', error);
    return null;
  }
}

/**
 * Sync local AIs to cloud (called when user logs in)
 * This is a placeholder for Phase 2 when we implement cloud sync
 */
export async function syncLocalAisToCloud(
  userId: string,
  uploadFn: (ai: UserDefinedAI) => Promise<string>
): Promise<{ synced: number; failed: number }> {
  try {
    const store = await getStore();
    const localAis = await getLocalCustomAis();
    
    // Filter AIs that haven't been synced yet (have "local-" prefix)
    const unsyncedAis = localAis.filter(ai => ai.id.startsWith('local-'));
    
    if (unsyncedAis.length === 0) {
      console.log('[LocalAiStorage] No unsynced AIs to upload');
      return { synced: 0, failed: 0 };
    }
    
    console.log(`[LocalAiStorage] Syncing ${unsyncedAis.length} local AIs to cloud...`);
    
    let syncedCount = 0;
    let failedCount = 0;
    const syncedAis: UserDefinedAI[] = [];
    
    for (const localAi of unsyncedAis) {
      try {
        // Upload to cloud and get the new cloud ID
        const cloudId = await uploadFn(localAi);
        
        // Update the local AI with the cloud ID
        const syncedAi: UserDefinedAI = {
          ...localAi,
          id: cloudId
        };
        
        syncedAis.push(syncedAi);
        syncedCount++;
        console.log(`[LocalAiStorage] Synced AI: ${localAi.name} (${localAi.id} → ${cloudId})`);
      } catch (error) {
        console.error(`[LocalAiStorage] Failed to sync AI: ${localAi.name}`, error);
        // Keep the local version if sync fails
        syncedAis.push(localAi);
        failedCount++;
      }
    }
    
    // Replace local AIs with synced versions
    const keptAis = localAis.filter(ai => !ai.id.startsWith('local-'));
    const allAis = [...keptAis, ...syncedAis];
    
    await store.set(CUSTOM_AIS_KEY, allAis);
    await store.set(SYNC_STATUS_KEY, {
      lastSyncTime: new Date().toISOString(),
      userId
    });
    await store.save();
    
    console.log(`[LocalAiStorage] Sync completed: ${syncedCount} synced, ${failedCount} failed`);
    return { synced: syncedCount, failed: failedCount };
    
  } catch (error) {
    console.error('[LocalAiStorage] Error syncing to cloud:', error);
    throw new Error('Failed to sync local AIs to cloud');
  }
}

/**
 * Merge cloud AIs into local storage (called after login)
 */
export async function mergeCloudAisToLocal(cloudAis: UserDefinedAI[]): Promise<void> {
  try {
    const store = await getStore();
    const localAis = await getLocalCustomAis();
    
    // Keep local-only AIs and add/update cloud AIs
    const localOnlyAis = localAis.filter(ai => ai.id.startsWith('local-'));
    // const _cloudAiIds = new Set(cloudAis.map(ai => ai.id));
    
    // Combine: local-only AIs + all cloud AIs
    const mergedAis = [...localOnlyAis, ...cloudAis];
    
    await store.set(CUSTOM_AIS_KEY, mergedAis);
    await store.save();
    
    console.log(`[LocalAiStorage] Merged ${cloudAis.length} cloud AIs with ${localOnlyAis.length} local-only AIs`);
  } catch (error) {
    console.error('[LocalAiStorage] Error merging cloud AIs:', error);
    throw new Error('Failed to merge cloud AIs to local storage');
  }
}

/**
 * Clear all locally stored AIs (use when logging out or resetting)
 */
export async function clearLocalAis(): Promise<void> {
  try {
    const store = await getStore();
    await store.set(CUSTOM_AIS_KEY, []);
    await store.set(SYNC_STATUS_KEY, null);
    await store.save();
    
    console.log('[LocalAiStorage] Cleared all local AIs');
  } catch (error) {
    console.error('[LocalAiStorage] Error clearing local AIs:', error);
    throw new Error('Failed to clear local AIs');
  }
}

/**
 * Get count of unsynced (local-only) AIs
 */
export async function getUnsyncedAisCount(): Promise<number> {
  try {
    const localAis = await getLocalCustomAis();
    const unsyncedCount = localAis.filter(ai => ai.id.startsWith('local-')).length;
    return unsyncedCount;
  } catch (error) {
    console.error('[LocalAiStorage] Error getting unsynced count:', error);
    return 0;
  }
}
