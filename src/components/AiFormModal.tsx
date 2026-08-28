import {
  component$,
  useStore,
  useSignal,
  useVisibleTask$,
  $,
  type QRL,
} from '@builder.io/qwik';
import type { UserDefinedAI, CreateUserAiData, UpdateUserAiData, LocalModel, LengthDisposition, TurnMode } from '../types';
import { getCachedModels, refreshLocalModels, refreshFits, refreshOnlineModels } from '../utils/modelCache';

/** Per-AI default turn mode (Edit-AI dropdown). */
const DEFAULT_MODE_OPTIONS: { id: TurnMode; name: string; description: string }[] = [
  { id: 'chat', name: 'Chat', description: 'Normal conversational replies' },
  { id: 'report', name: 'Report', description: 'Always a structured, reasoned report' },
  { id: 'code', name: 'Code', description: 'Always code-first' },
];
import { useAiData, useAiDataActions } from '../contexts/AiDataContext';
import {
  LuX,
  LuSave,
  LuAlertTriangle,
  LuLoader2,
  LuUploadCloud,
  LuRefreshCw,
  LuImage,
  LuChevronDown,
  LuCheck,
  LuUser,
  LuSlidersHorizontal,
  LuBookOpen,
  LuImagePlus,
  LuFileText,
  LuPuzzle,
  LuWrench,
  LuGlobe,
  LuFileDown,
  LuLock,
} from '@qwikest/icons/lucide';
import { invoke } from '@tauri-apps/api/core';
import { richModelName } from '../utils/modelNameFormatter';
import { isModelPaused } from '../utils/modelPrefs';
import { autoOptions, offeredOnlineModels } from '../utils/modelOptions';
import { ImageCropModal } from './ImageCropModal';
import { KnowledgeSection } from './KnowledgeSection';
import { listSkills, tokensLabel, type SkillInfo } from '../utils/skills';
import { listMcpServers, mcpSummary, type McpServer } from '../utils/mcp';
import { ThumbnailGalleryModal } from './ThumbnailGalleryModal';
import type { GalleryThumb } from '../data/thumbnail-gallery';
import { buildAiPack, signAiPack, aiPackFilename } from '../utils/aiPack';
import { vaultState, signInToFlowsta } from '../utils/packSigning';
import { LICENSES, currentMaker, shareCharacter, shareErrorText, type ShareResult } from '../utils/share';
import { rememberShare, rememberedShare, fetchShareStatus, shareStatusText, type ShareStatus } from '../utils/shareStatus';
import { getAiKnowledge } from '../utils/transcriptMemory';
import { LiquidMetalBorder } from './LiquidMetalBorder';
import LiquidMetalButton from './LiquidMetalButton';

interface AiFormModalProps {
  isOpen: boolean;
  onClose$: QRL<() => void>;
  editingAi: UserDefinedAI | null;
  currentDisplayableThumbnailUrl?: string | null;
}

// Static local placeholder
const GENERIC_PLACEHOLDER_IMG = '/generic-ai-placeholder.svg';

// Sanitizer function to prevent XSS
const sanitizeImageUrl = (url: string | null | undefined): string => {
  if (!url) return GENERIC_PLACEHOLDER_IMG;
  const trimmedUrl = url.trim();
  if (trimmedUrl.startsWith('blob:') || trimmedUrl.startsWith('/')) {
    return trimmedUrl;
  }
  return GENERIC_PLACEHOLDER_IMG;
};

const AiFormModal = component$<AiFormModalProps>(
  ({ isOpen, onClose$, editingAi, currentDisplayableThumbnailUrl }) => {
    const aiData = useAiData();
    const { addUserAi, editUserAi, refreshThumbnail } = useAiDataActions();

    const store = useStore({
      name: '',
      baseArchetypeId: '',
      description: '',
      isDescriptionCustomized: false,
      lengthDisposition: 'conversational' as LengthDisposition,
      defaultMode: 'chat' as TurnMode,
      systemPrompt: '',
      model: 'auto:offline',
      askBlurb: '',
      useEmojis: false,
      localModels: [] as LocalModel[],
      onlineModels: [] as { id: string; display_name: string; description: string }[],
      // Models served by the user's connected external engine (Settings → Engines).
      externalModels: [] as string[],
      // May online options be OFFERED for new selections? (signed in + plan;
      // starts true so a slow check never hides options from a paying user.)
      onlineEntitled: true,
      // model filename → fit grade (how well it runs on this device)
      fits: {} as Record<string, 'green' | 'split' | 'yellow' | 'red'>,
      // true while the offline list is being fetched with nothing cached to show yet
      modelsLoading: false,
      useArchetypeThumbnail: false,
      localPreviewOverride: null as string | null,
      originalImageSrc: null as string | null,
      showCropModal: false,
      activeSection: 'basics' as 'basics' | 'behaviour' | 'details' | 'knowledge' | 'skills' | 'appearance' | 'tools',
      // Skills chosen for this AI (none unless chosen).
      skills: [] as string[],
      mcp: [] as string[],
      installedMcp: [] as McpServer[],
      installedSkills: [] as SkillInfo[],
      skillFilter: '',
      knowledgeDocs: [] as import('../utils/transcriptMemory').KnowledgeDocument[],
      knowledgeBusy: false,
      knowledgeError: '' as string,
      showGalleryModal: false,
      // Bundled path of the gallery thumb picked this session (selected ring)
      galleryPath: null as string | null,
      formError: null as string | null,
      isSubmitting: false,
    });

    // We keep thumbnailFile in a signal since File objects are non-serializable
    // Installed skills for the Skills section (the list itself lives in Add-ons).
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async ({ track }) => {
      track(() => isOpen);
      if (!isOpen) return;
      store.installedSkills = await listSkills();
      store.installedMcp = await listMcpServers();
    });

    const thumbnailFile = useSignal<File | null>(null);
    const previewSrc = useSignal<string | null>(null);

    // Dropdown open states
    const personalityDropdownOpen = useSignal(false);
    const responseLengthDropdownOpen = useSignal(false);
    const defaultModeDropdownOpen = useSignal(false);
    const modelDropdownOpen = useSignal(false);

    const closeAllDropdowns = $((except?: string) => {
      if (except !== 'personality') personalityDropdownOpen.value = false;
      if (except !== 'responseLength') responseLengthDropdownOpen.value = false;
      if (except !== 'defaultMode') defaultModeDropdownOpen.value = false;
      if (except !== 'model') modelDropdownOpen.value = false;
    });

    // The edit dialog's content pane scrolls (max-h). A dropdown opened near
    // its bottom renders into the hidden overflow and looks like nothing
    // happened - scroll the menu into view once it's in the DOM.
    // block:'nearest' moves the pane the minimum needed; menus with room
    // don't move at all.
    const revealDropdown = $((menuId: string) => {
      setTimeout(() => {
        document
          .getElementById(menuId)
          ?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }, 60);
    });

    // Get all archetype templates (all personalities available)
    const getArchetypeTemplates = () => {
      const raw = aiData.archetypeTemplates;
      return Array.isArray(raw) ? raw : [];
    };

    // Generate default description
    const generateDefaultDescription = (aiName: string, archetypeId: string): string => {
      if (!aiName.trim() || !archetypeId) return '';
      const templates = getArchetypeTemplates();
      const archetype = templates.find((a) => a.id === archetypeId);
      const archetypeName = archetype ? archetype.name.toLowerCase() : 'archetype';
      const article = /^[aeiou]/.test(archetypeName) ? 'an' : 'a';
      return `${aiName} is my custom AI with the personality of ${article} ${archetypeName}.`;
    };

    // Fetch local models
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track }) => {
      track(() => isOpen);
      if (!isOpen) return;

      // Seed instantly from the session cache, then revalidate in the background —
      // so a cold open shows a spinner (not a misleading "No models available") and
      // a warm re-open is instant.
      const cached = getCachedModels();
      if (cached.local) store.localModels = cached.local;
      if (cached.fits) store.fits = cached.fits;
      if (cached.online) store.onlineModels = cached.online;
      // Only show the loading state when there's nothing cached to display yet.
      store.modelsLoading = !cached.local;

      refreshOnlineModels()
        .then((models) => { store.onlineModels = models; })
        .catch(() => { /* offline or proxy unreachable — online section just hides */ });

      // The user's own connected server (Settings → Engines), if healthy.
      invoke<{ healthy: boolean; models: string[] }>('external_engine_info')
        .then((info) => { store.externalModels = info.healthy ? info.models : []; })
        .catch(() => { store.externalModels = []; });

      // Online options are only OFFERED with a plan (existing online/auto
      // selections stay visible either way — billing is enforced per request).
      import('../utils/entitlement')
        .then(({ getOnlineEntitlement }) => getOnlineEntitlement())
        .then((e) => { store.onlineEntitled = e.entitled; })
        .catch(() => { /* keep fail-open default */ });

      // How well each offline model runs on this device (green/yellow/red dots).
      refreshFits()
        .then((fits) => { store.fits = fits; })
        .catch(() => { /* fit unavailable — dots just don't show */ });

      refreshLocalModels()
        .then((models) => {
          store.localModels = models;
          store.modelsLoading = false;
          // Online models ("online:") and Auto modes ("auto:") aren't local
          // files — don't treat them as a deleted local model and reset to the
          // first offline model.
          const isSpecial =
            !!store.model &&
            (store.model.startsWith('online:') ||
              store.model.startsWith('auto:') ||
              store.model.startsWith('external:'));
          const modelExists =
            store.model && (isSpecial || models.some((m) => m.name === store.model));
          if (store.model && !modelExists && models.length > 0) {
            // The saved model file is gone — fall back to Auto (offline), which
            // is always valid and lets the router pick the best available model.
            store.model = 'auto:offline';
          }
        })
        .catch((err) => {
          store.modelsLoading = false;
          console.error('Failed to load models:', err);
        });
    });

    // Body scroll lock
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      track(() => isOpen);
      if (isOpen) {
        document.body.classList.add('modal-open');
      } else {
        document.body.classList.remove('modal-open');
      }
      cleanup(() => {
        document.body.classList.remove('modal-open');
      });
    });

    // Export AI - the whole character as a signed pack (file or directory).
    const exportOpen = useSignal(false);
    const exportBusy = useSignal(false);
    const exportErr = useSignal('');
    const exportNote = useSignal('');
    const exportVaultUnlocked = useSignal(false);
    const exportVaultInstalled = useSignal(false);
    const shareMakerHandle = useSignal<string | null>(null);
    // What the reader clicked while the Vault was not ready; runs once it is.
    const exportIntent = useSignal<'share' | 'file' | null>(null);
    // Where an earlier share of this AI got to (read from GitHub when the form opens).
    const shareStatus = useSignal<ShareStatus | null>(null);
    const loadShareStatus = $(async () => {
      shareStatus.value = null;
      if (!editingAi) return;
      const r = rememberedShare('character', editingAi.id);
      if (!r) return;
      shareStatus.value = await fetchShareStatus(r);
    });

    const buildPack = $(async () => {
      const ai = editingAi!;
      let thumb: string | null = null;
      try {
        const bytes = await invoke<number[]>('get_ai_thumbnail', { aiId: ai.id });
        if (bytes?.length) {
          const { thumbnailBytesToDataUrl } = await import('../utils/aiPack');
          thumb = thumbnailBytesToDataUrl(bytes);
        }
      } catch { /* no portrait */ }
      const knowledge = (await getAiKnowledge(ai.id)).map((e) => ({ text: e.text }));
      return buildAiPack(ai, thumb, knowledge);
    });

    const exportChecking = useSignal(false);
    const openExport = $(async () => {
      exportErr.value = '';
      exportNote.value = '';
      exportIntent.value = null;
      exportOpen.value = true;
      exportChecking.value = true;
      try {
        const vs = await vaultState();
        exportVaultInstalled.value = vs.installed;
        exportVaultUnlocked.value = vs.unlocked;
        shareMakerHandle.value = (await currentMaker())?.handle ?? null;
      } catch (e) {
        exportErr.value = e instanceof Error ? e.message : String(e);
      } finally {
        exportChecking.value = false;
      }
    });

    const doExport = $(async () => {
      exportBusy.value = true;
      exportErr.value = '';
      try {
        let pack = await buildPack();
        pack = { ...pack, signature: await signAiPack(pack) };
        const path = await invoke<string>('save_text_download', {
          filename: aiPackFilename(pack),
          content: JSON.stringify(pack, null, 2),
        });
        exportOpen.value = false;
        exportNote.value = `Signed pack saved to ${path}`;
      } catch (e) {
        const m = e instanceof Error ? e.message : String(e);
        exportErr.value =
          m === 'vault_locked'
            ? 'Flowsta Vault is locked - unlock it to sign.'
            : m === 'vault_not_found'
              ? 'Flowsta Vault isn\'t running.'
              : 'Couldn\'t save the pack.';
      } finally {
        exportBusy.value = false;
      }
    });

    // Share to the directory: the signed pack + a listing, filed as a pull
    // request by the share service under the maker's Flowsta name.
    const shareOpen = useSignal(false);
    const shareBusy = useSignal(false);
    const shareErr = useSignal('');
    const shareDone = useSignal<ShareResult | null>(null);
    const shareDescription = useSignal('');
    const shareTitle = useSignal('');
    const shareLicense = useSignal('CC-BY-4.0');
    const shareLicenseOpen = useSignal(false);
    const openShare = $(async () => {
      shareErr.value = '';
      shareDone.value = null;
      shareDescription.value = editingAi?.description || '';
      shareTitle.value = '';
      shareLicenseOpen.value = false;
      const maker = await currentMaker();
      shareMakerHandle.value = maker?.handle ?? null;
      const vs = await vaultState();
      exportVaultInstalled.value = vs.installed;
      exportVaultUnlocked.value = vs.unlocked;
      shareOpen.value = true;
    });
    const doShare = $(async () => {
      shareBusy.value = true;
      shareErr.value = '';
      try {
        const maker = await currentMaker();
        if (!maker) throw new Error('Sign in with Flowsta first - a share carries your name.');
        const desc = shareDescription.value.trim();
        if (desc.length < 20) throw new Error('Say a little more about it - at least a sentence.');
        let pack = await buildPack();
        pack = { ...pack, description: desc, signature: undefined as never };
        pack = { ...pack, signature: await signAiPack(pack) };
        shareDone.value = await shareCharacter(pack, { description: desc, license: shareLicense.value, title: shareTitle.value.trim() || undefined, maker });
        rememberShare('character', editingAi!.id, shareDone.value);
        shareStatus.value = { state: 'checking', page: shareDone.value.page, pr_url: shareDone.value.pr_url };
      } catch (e) {
        shareErr.value = shareErrorText(e);
      } finally {
        shareBusy.value = false;
      }
    });

    const doExportSignIn = $(async () => {
      exportBusy.value = true;
      exportErr.value = '';
      try {
        await signInToFlowsta();
        const vs = await vaultState();
        exportVaultInstalled.value = vs.installed;
        exportVaultUnlocked.value = vs.unlocked;
        shareMakerHandle.value = (await currentMaker())?.handle ?? null;
        if (vs.unlocked && exportIntent.value === 'file') { exportIntent.value = null; await doExport(); }
        else if (vs.unlocked && exportIntent.value === 'share') { exportIntent.value = null; exportOpen.value = false; await openShare(); }
      } catch {
        exportErr.value = 'Sign-in was cancelled or didn\'t complete.';
      } finally {
        exportBusy.value = false;
      }
    });

    // Initialize/reset form state when modal opens or editingAi changes
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      track(() => isOpen);
      track(() => editingAi);

      if (isOpen) {
        store.isSubmitting = false;
        store.formError = null;
        thumbnailFile.value = null;
        store.localPreviewOverride = null;
        store.useArchetypeThumbnail = false;
        store.galleryPath = null;
        previewSrc.value = null;

        const templates = getArchetypeTemplates();
        store.skills = [];
        store.mcp = [];

        if (editingAi) {
          void loadShareStatus();
          store.name = editingAi.name;
          store.baseArchetypeId = editingAi.baseArchetypeId;
          store.systemPrompt = editingAi.systemPrompt || '';
          store.lengthDisposition = editingAi.lengthDisposition || 'conversational';
          store.defaultMode = editingAi.defaultMode || 'chat';
          store.model = editingAi.model || 'auto:offline';
          store.askBlurb = editingAi.askBlurb || '';
          store.useEmojis = editingAi.useEmojis ?? false;
          store.skills = Array.isArray(editingAi.skills) ? [...editingAi.skills] : [];
          store.mcp = Array.isArray(editingAi.mcp) ? [...editingAi.mcp] : [];

          const defaultDescription = generateDefaultDescription(
            editingAi.name,
            editingAi.baseArchetypeId
          );
          const currentDescription = editingAi.description || defaultDescription;
          store.description = currentDescription;
          store.isDescriptionCustomized = currentDescription !== defaultDescription;

          if (!currentDisplayableThumbnailUrl) {
            store.useArchetypeThumbnail = true;
          }
        } else {
          store.name = '';
          const defaultArchetype = templates.length > 0 ? templates[0] : null;
          store.baseArchetypeId = defaultArchetype?.id || '';
          store.systemPrompt = defaultArchetype?.systemPromptTemplate || '';
          store.description = '';
          store.lengthDisposition = 'conversational';
          store.defaultMode = 'chat';
          // New AIs default to Auto (offline) — always valid, router-picked.
          store.model = 'auto:offline';
          store.askBlurb = '';
          store.useEmojis = false;
          store.isDescriptionCustomized = false;
          store.useArchetypeThumbnail = true;
        }
      } else {
        // Cleanup local blob URL when modal closes
        if (store.localPreviewOverride && store.localPreviewOverride.startsWith('blob:')) {
          URL.revokeObjectURL(store.localPreviewOverride);
        }
        store.localPreviewOverride = null;
      }

      cleanup(() => {
        if (store.localPreviewOverride && store.localPreviewOverride.startsWith('blob:')) {
          URL.revokeObjectURL(store.localPreviewOverride);
        }
      });
    });

    // Update system prompt when archetype changes (new AI only)
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track }) => {
      const archetypeId = track(() => store.baseArchetypeId);
      if (archetypeId && !editingAi) {
        const templates = getArchetypeTemplates();
        const archetype = templates.find((a) => a.id === archetypeId);
        if (archetype) {
          store.systemPrompt = archetype.systemPromptTemplate;
        }
      }
    });

    // While in "match personality" mode, keep the preview following the
    // selected personality's art (a gallery pick or upload overrides this).
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track }) => {
      const archetypeId = track(() => store.baseArchetypeId);
      if (!archetypeId || thumbnailFile.value || !store.useArchetypeThumbnail) return;
      const archetypeUrl = aiData.archetypeDefaultThumbnailsMap[archetypeId];
      if (previewSrc.value) {
        previewSrc.value = archetypeUrl || GENERIC_PLACEHOLDER_IMG;
      }
    });

    // Auto-update description when name or personality changes
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track }) => {
      const name = track(() => store.name);
      const archetypeId = track(() => store.baseArchetypeId);
      const isCustomized = track(() => store.isDescriptionCustomized);

      if (!isCustomized && name.trim() && archetypeId) {
        store.description = generateDefaultDescription(name, archetypeId);
      }
    });

    // Handle file selection - open crop modal
    const handleFileChange$ = $((e: Event) => {
      const input = e.target as HTMLInputElement;
      const file = input.files?.[0];
      if (file) {
        if (file.size > 10 * 1024 * 1024) {
          store.formError = 'Thumbnail image must be less than 10MB.';
          return;
        }
        if (!file.type.startsWith('image/')) {
          store.formError = 'Please select an image file.';
          return;
        }
        store.formError = null;

        const reader = new FileReader();
        reader.onloadend = () => {
          store.originalImageSrc = reader.result as string;
          store.showCropModal = true;
        };
        reader.readAsDataURL(file);
      }
    });

    // Handle cropped image from modal
    const handleCroppedImage$ = $((croppedImageBlob: Blob) => {
      const croppedFile = new File([croppedImageBlob], 'thumbnail.jpg', { type: 'image/jpeg' });
      thumbnailFile.value = croppedFile;

      // Create preview URL
      if (store.localPreviewOverride) {
        URL.revokeObjectURL(store.localPreviewOverride);
      }
      const objectUrl = URL.createObjectURL(croppedFile);
      store.localPreviewOverride = objectUrl;
      store.useArchetypeThumbnail = false;
      store.galleryPath = null;
      previewSrc.value = objectUrl;
    });

    // Handle gallery pick — saved through the same custom-thumbnail path as
    // an upload, so it sticks across personality changes.
    const handleGallerySelect$ = $(async (thumb: GalleryThumb) => {
      store.showGalleryModal = false;
      try {
        const response = await fetch(thumb.path);
        const blob = await response.blob();
        thumbnailFile.value = new File([blob], 'thumbnail.jpg', { type: 'image/jpeg' });
        if (store.localPreviewOverride) {
          URL.revokeObjectURL(store.localPreviewOverride);
          store.localPreviewOverride = null;
        }
        store.useArchetypeThumbnail = false;
        store.galleryPath = thumb.path;
        store.formError = null;
        previewSrc.value = thumb.path;
      } catch (err) {
        console.error('[AiFormModal] Failed to load gallery thumbnail:', err);
        store.formError = 'Failed to load that thumbnail. Please try another.';
      }
    });

    // Clear thumbnail — the AI follows its personality's art from here on
    const handleClearThumbnail$ = $(() => {
      if (store.localPreviewOverride) {
        URL.revokeObjectURL(store.localPreviewOverride);
      }
      thumbnailFile.value = null;
      store.localPreviewOverride = null;
      const fileInput = document.getElementById('thumbnailFile') as HTMLInputElement;
      if (fileInput) fileInput.value = '';
      store.useArchetypeThumbnail = true;
      store.galleryPath = null;
      store.formError = null;
      // Force preview to archetype default
      const archetypeUrl = aiData.archetypeDefaultThumbnailsMap[store.baseArchetypeId];
      previewSrc.value = archetypeUrl || GENERIC_PLACEHOLDER_IMG;
    });

    // Form submission
    const handleSubmit$ = $(async (e: SubmitEvent) => {
      e.preventDefault();
      if (store.isSubmitting) return;
      store.formError = null;

      if (!store.name.trim()) {
        store.formError = 'AI Name is required.';
        return;
      }
      if (!store.baseArchetypeId) {
        store.formError = 'Personality is required.';
        return;
      }
      if (!store.model) {
        store.formError = 'Please select a model.';
        return;
      }

      store.isSubmitting = true;

      try {
        let success = false;
        const commonData = {
          name: store.name.trim(),
          baseArchetypeId: store.baseArchetypeId,
          systemPrompt: store.systemPrompt.trim(),
          description: store.description.trim(),
          lengthDisposition: store.lengthDisposition,
          defaultMode: store.defaultMode,
          model: store.model,
          askBlurb: store.askBlurb.trim(),
          useEmojis: store.useEmojis,
          skills: store.skills,
          mcp: store.mcp,
        };

        if (editingAi) {
          const updateData: UpdateUserAiData = { ...commonData };

          if (thumbnailFile.value) {
            try {
              const fileBuffer = await thumbnailFile.value.arrayBuffer();
              const thumbnailPath = await invoke<string>('save_ai_thumbnail', {
                aiId: editingAi.id,
                thumbnailData: Array.from(new Uint8Array(fileBuffer)),
              });
              console.log('[AiFormModal] Thumbnail saved:', thumbnailPath);
            } catch (err) {
              console.error('[AiFormModal] Failed to save thumbnail:', err);
              store.formError = 'Failed to save thumbnail. Please try again.';
              store.isSubmitting = false;
              return;
            }
          } else if (store.useArchetypeThumbnail) {
            try {
              await invoke('delete_ai_thumbnail', { aiId: editingAi.id });
              console.log('[AiFormModal] Custom thumbnail deleted, reverting to archetype default');
            } catch (err) {
              console.log('[AiFormModal] No custom thumbnail to delete');
            }
          }

          const result = await editUserAi(editingAi.id, updateData);
          if (result) {
            success = true;
            await refreshThumbnail(editingAi.id);
          }
        } else {
          const createData: CreateUserAiData = { ...commonData };
          const result = await addUserAi(createData);
          if (result) {
            success = true;

            if (thumbnailFile.value && result.id) {
              try {
                const fileBuffer = await thumbnailFile.value.arrayBuffer();
                const thumbnailPath = await invoke<string>('save_ai_thumbnail', {
                  aiId: result.id,
                  thumbnailData: Array.from(new Uint8Array(fileBuffer)),
                });
                console.log('[AiFormModal] Thumbnail saved for new AI:', thumbnailPath);
                await refreshThumbnail(result.id);
              } catch (err) {
                console.error('[AiFormModal] Failed to save thumbnail for new AI:', err);
              }
            }
          }
        }

        if (success) {
          onClose$();
        } else {
          store.formError = aiData.userAisError || 'An unexpected error occurred. Please try again.';
        }
      } catch (err: any) {
        console.error('Error submitting AI form:', err);
        store.formError = err.message || 'Submission failed. Please check your connection and try again.';
      } finally {
        store.isSubmitting = false;
      }
    });

    if (!isOpen) return null;

    const archetypeTemplates = getArchetypeTemplates();

    return (
      <div
        class="fixed inset-0 bg-black/60 flex items-start justify-center p-4 z-50 transition-opacity duration-300 ease-in-out overflow-y-auto"
        onClick$={(e, el) => {
          // Qwik resets e.currentTarget to null in async handlers — use the
          // element arg so a backdrop click actually closes the modal.
          if (e.target === el) onClose$();
        }}
      >
        <div
          class={`bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full relative my-8 ${editingAi ? 'max-w-4xl' : 'max-w-lg'}`}
          onClick$={(e) => e.stopPropagation()}
        >
          {store.isSubmitting && (
            <div class="absolute inset-0 bg-[var(--bg-header-footer)] bg-opacity-70 flex items-center justify-center z-10 rounded-xl">
              <LuLoader2 class="h-10 w-10 animate-spin text-[var(--text-link)]" />
            </div>
          )}
          <div class="flex justify-between items-center mb-6">
            <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">
              {editingAi ? 'Edit AI' : 'Create New AI'}
            </h2>
            <button
              onClick$={onClose$}
              class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
              aria-label="Close modal"
              disabled={store.isSubmitting}
            >
              <LuX class="w-6 h-6" />
            </button>
          </div>

          <form preventdefault:submit onSubmit$={handleSubmit$} class={editingAi ? 'flex flex-col gap-5' : 'space-y-4 md:space-y-6'}>
            <div class={editingAi ? 'flex gap-6' : 'contents'}>
            {/* Section nav (edit only) - mirrors the Settings page pattern. */}
            {editingAi && (
              <nav class="shrink-0 w-44 space-y-1">
                {[
                  { id: 'basics', label: 'Basics', icon: LuUser },
                  { id: 'behaviour', label: 'Behaviour', icon: LuSlidersHorizontal },
                  { id: 'details', label: 'Details', icon: LuFileText },
                  { id: 'knowledge', label: 'Knowledge', icon: LuBookOpen },
                  { id: 'skills', label: 'Skills', icon: LuPuzzle },
                  { id: 'tools', label: 'Tools', icon: LuWrench },
                  { id: 'appearance', label: 'Appearance', icon: LuImagePlus },
                ].map((sec) => {
                  const Icon = sec.icon;
                  const active = store.activeSection === sec.id;
                  return (
                    <button
                      key={sec.id}
                      type="button"
                      onClick$={() => { store.activeSection = sec.id as typeof store.activeSection; }}
                      class={`flex items-center gap-2.5 w-full text-left px-3 py-2 rounded-lg text-sm transition-colors ${
                        active
                          ? 'bg-[var(--bg-button-primary)] text-[var(--text-button-primary)]'
                          : 'text-[var(--text-secondary)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)]'
                      }`}
                    >
                      <Icon class="w-4 h-4 shrink-0" />
                      {sec.label}
                    </button>
                  );
                })}
              </nav>
            )}

            {/* Section content (edit: one active pane; create: all stacked) */}
            <div class={editingAi ? 'flex-1 min-w-0 space-y-4 md:space-y-6 max-h-[65vh] overflow-y-auto pr-1' : 'contents'}>
            {(!editingAi || store.activeSection === 'basics') && (<>
            {/* AI Name */}
            <div>
              <div class="flex justify-between items-center mb-1">
                <label for="aiName" class="block text-sm font-medium text-[var(--text-secondary)]">
                  AI Name *
                </label>
                <span
                  class={`text-sm font-medium ${
                    store.name.length > 25 ? 'text-red-500' : 'text-[var(--text-muted)]'
                  }`}
                >
                  {store.name.length} / 25
                </span>
              </div>
              <LiquidMetalBorder borderRadius="9999px">
                <input
                  type="text"
                  id="aiName"
                  value={store.name}
                  onInput$={(e) => {
                    store.name = (e.target as HTMLInputElement).value;
                  }}
                  required
                  maxLength={25}
                  disabled={store.isSubmitting}
                  class="w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2.5 placeholder-[var(--text-muted)] transition-colors disabled:opacity-70 gradient-border-target"
                  placeholder="E.g., Marketing Guru Bot"
                />
              </LiquidMetalBorder>
            </div>

            {/* Personality */}
            <div>
              <label
                for="baseArchetypeId"
                class="block text-sm font-medium text-[var(--text-secondary)] mb-1"
              >
                Personality *
              </label>
              <LiquidMetalBorder borderRadius="9999px">
                <div class="relative">
                  <button
                    type="button"
                    class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2.5 pl-4 pr-10 text-left text-[var(--text-primary)] focus:outline-none disabled:opacity-70 gradient-border-target"
                    disabled={store.isSubmitting}
                    onClick$={() => {
                      closeAllDropdowns('personality');
                      personalityDropdownOpen.value = !personalityDropdownOpen.value;
                      if (personalityDropdownOpen.value) revealDropdown('ai-dd-personality');
                    }}
                  >
                    <span class="block truncate">
                      {archetypeTemplates.find(a => a.id === store.baseArchetypeId)?.name || 'Select...'}
                    </span>
                    <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                      <LuChevronDown class="h-5 w-5 text-[var(--text-muted)]" aria-hidden="true" />
                    </span>
                  </button>
                  {personalityDropdownOpen.value && (
                    <ul id="ai-dd-personality" class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                      {archetypeTemplates.map((arch) => (
                        <li
                          key={arch.id}
                          class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                            store.baseArchetypeId === arch.id
                              ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                              : 'text-[var(--text-dropdown)]'
                          }`}
                          onClick$={() => {
                            store.baseArchetypeId = arch.id;
                            personalityDropdownOpen.value = false;
                          }}
                        >
                          <span class={`block truncate ${store.baseArchetypeId === arch.id ? 'font-medium' : 'font-normal'}`}>
                            {arch.name}
                          </span>
                          {store.baseArchetypeId === arch.id && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-5 w-5" aria-hidden="true" />
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </LiquidMetalBorder>
            </div>

            </>)}
            {(!editingAi || store.activeSection === 'behaviour') && (<>
            {/* Response Length + Default Mode (edit only) */}
            {editingAi && (
              <>
              <div>
                <label class="block text-sm font-medium text-[var(--text-secondary)] mb-1">
                  Response Length *
                </label>
                <LiquidMetalBorder borderRadius="9999px">
                  <div class="relative">
                    <button
                      type="button"
                      class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2.5 pl-4 pr-10 text-left text-[var(--text-primary)] focus:outline-none disabled:opacity-70 gradient-border-target"
                      disabled={store.isSubmitting}
                      onClick$={() => {
                        closeAllDropdowns('responseLength');
                        responseLengthDropdownOpen.value = !responseLengthDropdownOpen.value;
                      if (responseLengthDropdownOpen.value) revealDropdown('ai-dd-length');
                      }}
                    >
                      <span class="block truncate">
                        {aiData.responseLengthOptions.find(
                          (opt) => opt.id === store.lengthDisposition
                        )?.name || 'Select...'}
                      </span>
                      <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                        <LuChevronDown
                          class="h-5 w-5 text-[var(--text-muted)]"
                          aria-hidden="true"
                        />
                      </span>
                    </button>
                    {responseLengthDropdownOpen.value && (
                      <ul id="ai-dd-length" class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                        {aiData.responseLengthOptions.map((option) => (
                          <li
                            key={option.id}
                            class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                              store.lengthDisposition === option.id
                                ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                                : 'text-[var(--text-dropdown)]'
                            }`}
                            onClick$={() => {
                              store.lengthDisposition = option.id;
                              responseLengthDropdownOpen.value = false;
                            }}
                          >
                            <span
                              class={`block truncate ${
                                store.lengthDisposition === option.id ? 'font-medium' : 'font-normal'
                              }`}
                            >
                              {option.name}
                            </span>
                            {store.lengthDisposition === option.id && (
                              <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                                <LuCheck class="h-5 w-5" aria-hidden="true" />
                              </span>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </LiquidMetalBorder>
              </div>

              {/* Default Mode — what kind of answer this AI gives by default. */}
              <div>
                <label class="block text-sm font-medium text-[var(--text-secondary)] mb-1">
                  Default Mode
                </label>
                <LiquidMetalBorder borderRadius="9999px">
                  <div class="relative">
                    <button
                      type="button"
                      class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2.5 pl-4 pr-10 text-left text-[var(--text-primary)] focus:outline-none disabled:opacity-70 gradient-border-target"
                      disabled={store.isSubmitting}
                      onClick$={() => {
                        closeAllDropdowns('defaultMode');
                        defaultModeDropdownOpen.value = !defaultModeDropdownOpen.value;
                      if (defaultModeDropdownOpen.value) revealDropdown('ai-dd-mode');
                      }}
                    >
                      <span class="block truncate">
                        {DEFAULT_MODE_OPTIONS.find((opt) => opt.id === store.defaultMode)?.name || 'Chat'}
                      </span>
                      <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                        <LuChevronDown class="h-5 w-5 text-[var(--text-muted)]" aria-hidden="true" />
                      </span>
                    </button>
                    {defaultModeDropdownOpen.value && (
                      <ul id="ai-dd-mode" class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                        {DEFAULT_MODE_OPTIONS.map((option) => (
                          <li
                            key={option.id}
                            class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                              store.defaultMode === option.id
                                ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                                : 'text-[var(--text-dropdown)]'
                            }`}
                            onClick$={() => {
                              store.defaultMode = option.id;
                              defaultModeDropdownOpen.value = false;
                            }}
                          >
                            <span class={`block truncate ${store.defaultMode === option.id ? 'font-medium' : 'font-normal'}`}>
                              {option.name}
                            </span>
                            {store.defaultMode === option.id && (
                              <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                                <LuCheck class="h-5 w-5" aria-hidden="true" />
                              </span>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </LiquidMetalBorder>
                <p class="text-xs text-[var(--text-muted)] mt-1">
                  Most AIs stay on Chat. Set Report for an AI that should always write structured reports, or Code for a coding assistant.
                </p>
              </div>
              </>
            )}

            </>)}
            {(!editingAi || store.activeSection === 'basics') && (<>
            {/* Base Model */}
            <div>
              <label class="block text-sm font-medium text-[var(--text-secondary)] mb-1">
                Base Model *
              </label>
              <LiquidMetalBorder borderRadius="9999px">
                <div class="relative">
                  <button
                    type="button"
                    class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2.5 pl-4 pr-10 text-left text-[var(--text-primary)] focus:outline-none disabled:opacity-70 gradient-border-target"
                    disabled={store.isSubmitting || store.modelsLoading || (store.localModels.length === 0 && store.onlineModels.length === 0 && store.externalModels.length === 0)}
                    onClick$={() => {
                      closeAllDropdowns('model');
                      modelDropdownOpen.value = !modelDropdownOpen.value;
                      if (modelDropdownOpen.value) revealDropdown('ai-dd-model');
                    }}
                  >
                    <span class="flex items-center gap-2 truncate">
                      {store.modelsLoading && store.localModels.length === 0 ? (
                        <>
                          <span class="w-3.5 h-3.5 border-2 border-[var(--text-muted)] border-t-transparent rounded-full animate-spin flex-shrink-0" />
                          <span class="text-[var(--text-muted)]">Loading your models…</span>
                        </>
                      ) : store.localModels.length === 0 && store.onlineModels.length === 0 && store.externalModels.length === 0
                        ? 'No models available'
                        : !store.model || store.model === ''
                          ? 'Select a model...'
                          : store.model === 'auto:offline'
                            ? 'Auto — Offline Only'
                            : store.model === 'auto:online-offline'
                              ? 'Auto — Online and Offline'
                              : store.model === 'auto:my-hardware'
                                ? 'Auto — My Hardware'
                                : store.model.startsWith('online:')
                                ? `${store.onlineModels.find((m) => m.id === store.model)?.display_name || store.model.slice(7)} (online)`
                                : store.model.startsWith('external:')
                                  ? `${store.model.slice(9)} (your server)`
                                  : richModelName(store.model)}
                    </span>
                    <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                      <LuChevronDown class="h-5 w-5 text-[var(--text-muted)]" aria-hidden="true" />
                    </span>
                  </button>
                  {modelDropdownOpen.value && (
                    <ul id="ai-dd-model" class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                      {/* Smart routing — Auto modes pick the model per question */}
                      <li class="select-none pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                        Smart routing
                      </li>
                      {/* Option rules shared with the ModelChip surfaces -
                          see utils/modelOptions. */}
                      {autoOptions({
                        hasExternal: store.externalModels.length > 0,
                        hasOnlineModels: store.onlineModels.length > 0,
                        onlineEntitled: store.onlineEntitled,
                        currentModel: store.model,
                      }).map((opt) => (
                        <li
                          key={opt.id}
                          class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                            store.model === opt.id
                              ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                              : 'text-[var(--text-dropdown)]'
                          }`}
                          onClick$={() => {
                            store.model = opt.id;
                            modelDropdownOpen.value = false;
                          }}
                        >
                          <span class={`block truncate ${store.model === opt.id ? 'font-medium' : 'font-normal'}`}>
                            {opt.label}
                            <span class="ml-2 text-xs text-[var(--text-muted)]">{opt.hint}</span>
                          </span>
                          {store.model === opt.id && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-5 w-5" aria-hidden="true" />
                            </span>
                          )}
                        </li>
                      ))}
                      {store.localModels.length > 0 && (
                        <li class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                          Your offline models
                        </li>
                      )}
                      {store.localModels
                        .filter((m) => !isModelPaused(m.name) || m.name === store.model)
                        .map((localModel) => (
                        <li
                          key={localModel.name}
                          class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                            store.model === localModel.name
                              ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                              : 'text-[var(--text-dropdown)]'
                          }`}
                          onClick$={() => {
                            store.model = localModel.name;
                            modelDropdownOpen.value = false;
                          }}
                        >
                          <span
                            class={`block truncate ${
                              store.model === localModel.name ? 'font-medium' : 'font-normal'
                            }`}
                          >
                            {(() => {
                              const fit = store.fits[localModel.name];
                              if (!fit) return null;
                              const color =
                                fit === 'green' || fit === 'split'
                                  ? 'bg-green-500'
                                  : fit === 'yellow'
                                    ? 'bg-yellow-500'
                                    : 'bg-red-500';
                              const title =
                                fit === 'green'
                                  ? 'Fits fully on your GPU — fast'
                                  : fit === 'split'
                                    ? 'GPU + RAM — its experts run from main memory, fast for its size'
                                    : fit === 'yellow'
                                      ? 'Runs slower on this machine'
                                      : 'Needs more memory than this machine has';
                              return (
                                <span
                                  class={`inline-block w-2 h-2 rounded-full ${color} mr-2 align-middle`}
                                  title={title}
                                />
                              );
                            })()}
                            {richModelName(localModel.name)}
                            <span class="ml-2 text-xs text-[var(--text-muted)]">{localModel.size}</span>
                          </span>
                          {store.model === localModel.name && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-5 w-5" aria-hidden="true" />
                            </span>
                          )}
                        </li>
                      ))}
                      {store.onlineModels.length > 0 && !store.onlineEntitled && (
                        <li class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-2 pl-4 pr-4 text-xs text-[var(--text-muted)]">
                          Online models (and Auto — Online and Offline) unlock with a
                          plan — set up on the Online Models page.
                        </li>
                      )}
                      {store.onlineModels.length > 0 && store.onlineEntitled && (
                        <li class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                          Online models · Flowsta sign-in
                        </li>
                      )}
                      {offeredOnlineModels(store.onlineModels, store.onlineEntitled, store.model, isModelPaused)
                        .map((om) => (
                        <li
                          key={om.id}
                          class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                            store.model === om.id
                              ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                              : 'text-[var(--text-dropdown)]'
                          }`}
                          onClick$={() => {
                            store.model = om.id;
                            modelDropdownOpen.value = false;
                          }}
                        >
                          <span class={`block truncate ${store.model === om.id ? 'font-medium' : 'font-normal'}`}>
                            {om.display_name}
                            <span class="ml-2 text-xs text-[var(--text-muted)]">{om.description}</span>
                          </span>
                          {store.model === om.id && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-5 w-5" aria-hidden="true" />
                            </span>
                          )}
                        </li>
                      ))}
                      {store.externalModels.length > 0 && (
                        <li class="select-none border-t border-[var(--border-subtle)] mt-1 pt-2 pb-1 pl-4 pr-4 text-xs uppercase tracking-wider text-[var(--text-muted)]">
                          Your server · external engine
                        </li>
                      )}
                      {store.externalModels
                        .map((id) => `external:${id}`)
                        .filter((full) => !isModelPaused(full) || full === store.model)
                        .map((full) => (
                        <li
                          key={full}
                          class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                            store.model === full
                              ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                              : 'text-[var(--text-dropdown)]'
                          }`}
                          onClick$={() => {
                            store.model = full;
                            modelDropdownOpen.value = false;
                          }}
                        >
                          <span class={`block truncate ${store.model === full ? 'font-medium' : 'font-normal'}`}>
                            {full.slice(9)}
                            <span class="ml-2 text-xs text-[var(--text-muted)]">runs on your connected server</span>
                          </span>
                          {store.model === full && (
                            <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                              <LuCheck class="h-5 w-5" aria-hidden="true" />
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </LiquidMetalBorder>
            </div>

            </>)}
            {(!editingAi || store.activeSection === 'behaviour') && (<>
            {/* Use Emojis */}
            <div>
              <label class="block text-sm font-medium text-[var(--text-secondary)] mb-2">
                Use Emojis in Responses
              </label>
              <LiquidMetalBorder borderRadius="9999px">
                <div class="flex rounded-full overflow-hidden bg-[var(--bg-card)]">
                  <button
                    type="button"
                    onClick$={() => {
                      store.useEmojis = false;
                    }}
                    disabled={store.isSubmitting}
                    class={`flex-1 px-4 py-2.5 font-medium focus:outline-none transition-colors disabled:opacity-70
                      ${
                        !store.useEmojis
                          ? 'bg-[var(--bg-main)] text-[var(--text-primary)] font-semibold'
                          : 'text-[var(--text-muted)] hover:text-[var(--text-secondary)]'
                      }
                    `}
                  >
                    Off
                  </button>
                  <button
                    type="button"
                    onClick$={() => {
                      store.useEmojis = true;
                    }}
                    disabled={store.isSubmitting}
                    class={`flex-1 px-4 py-2.5 font-medium focus:outline-none transition-colors disabled:opacity-70
                      ${
                        store.useEmojis
                          ? 'bg-[var(--bg-main)] text-[var(--text-primary)] font-semibold'
                          : 'text-[var(--text-muted)] hover:text-[var(--text-secondary)]'
                      }
                    `}
                  >
                    On
                  </button>
                </div>
              </LiquidMetalBorder>
            </div>

            </>)}
            {(!editingAi || store.activeSection === 'details') && (<>
            {/* Description (edit only) */}
            {editingAi && (
              <div>
                <label
                  for="aiDescription"
                  class="block text-sm font-medium text-[var(--text-secondary)] mb-1"
                >
                  Description
                </label>
                <LiquidMetalBorder borderRadius="1rem">
                  <textarea
                    id="aiDescription"
                    value={store.description}
                    onInput$={(e) => {
                      store.description = (e.target as HTMLTextAreaElement).value;
                      store.isDescriptionCustomized = true;
                    }}
                    disabled={store.isSubmitting}
                    class="w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-2xl px-4 py-2.5 placeholder-[var(--text-muted)] transition-colors disabled:opacity-70 gradient-border-target border-none"
                    placeholder="A brief description of your AI."
                    rows={3}
                  />
                </LiquidMetalBorder>
              </div>
            )}

            {/* Ask Blurb (edit only) */}
            {editingAi && (
              <div>
                <div class="flex justify-between items-center mb-1">
                  <label
                    for="askBlurb"
                    class="block text-sm font-medium text-[var(--text-secondary)]"
                  >
                    Ask {store.name || '(AI Name)'} {store.askBlurb || '(description)'}..
                  </label>
                  <span
                    class={`text-sm font-medium ${
                      store.askBlurb.length > 25 ? 'text-red-500' : 'text-[var(--text-muted)]'
                    }`}
                  >
                    {store.askBlurb.length} / 25
                  </span>
                </div>
                <LiquidMetalBorder borderRadius="9999px">
                  <input
                    type="text"
                    id="askBlurb"
                    value={store.askBlurb}
                    onInput$={(e) => {
                      store.askBlurb = (e.target as HTMLInputElement).value;
                    }}
                    maxLength={25}
                    disabled={store.isSubmitting}
                    class="w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2.5 placeholder-[var(--text-muted)] transition-colors disabled:opacity-70 gradient-border-target"
                    placeholder="about marketing strategies"
                  />
                </LiquidMetalBorder>
              </div>
            )}

            </>)}
            {(!editingAi || store.activeSection === 'knowledge') && editingAi && (
              <KnowledgeSection aiId={editingAi.id} store={store} />
            )}
            {editingAi && store.activeSection === 'tools' && (
              <div class="space-y-3">
                <p class="block text-sm font-medium text-[var(--text-secondary)]">Tools</p>
                <p class="text-sm text-[var(--text-muted)]">
                  Programs this AI may use in a project: Blender, a browser, a printer, your smart home. Tick the ones this
                  AI should carry; every tool call goes through your approve step, like a file edit.
                </p>
                {store.installedMcp.length === 0 ? (
                  <p class="text-sm text-[var(--text-secondary)]">
                    No tools added yet.{' '}
                    <a href="/add-ons/mcp" class="text-[var(--text-link)] hover:underline">Add some in Add-ons</a>.
                  </p>
                ) : (
                  <>
                    <p class="text-xs text-[var(--text-muted)]">
                      {store.mcp.length} of {store.installedMcp.length} chosen · used in projects only for now
                    </p>
                    <div class="max-h-64 overflow-y-auto space-y-1.5 pr-1">
                      {store.installedMcp.map((t) => {
                        const on = store.mcp.includes(t.name);
                        return (
                          <label key={t.name} class="flex items-start gap-2 text-sm text-[var(--text-primary)]">
                            <input
                              type="checkbox"
                              checked={on}
                              onChange$={() => {
                                store.mcp = on ? store.mcp.filter((n) => n !== t.name) : [...store.mcp, t.name];
                              }}
                              class="mt-0.5"
                            />
                            <span class="min-w-0">
                              <span class="font-medium">{t.name}</span>
                              {t.description && (
                                <span class="block text-xs text-[var(--text-secondary)] line-clamp-2">{t.description}</span>
                              )}
                              <span class="block text-xs text-[var(--text-muted)] truncate">{mcpSummary(t)}</span>
                            </span>
                          </label>
                        );
                      })}
                    </div>
                  </>
                )}
              </div>
            )}
            {editingAi && store.activeSection === 'skills' && (
              <div class="space-y-3">
                <p class="block text-sm font-medium text-[var(--text-secondary)]">Skills</p>
                <p class="text-sm text-[var(--text-muted)]">
                  What this AI knows how to do. Tick the skills this AI should use; in chat the one that fits the
                  question is brought in, so a few chosen skills stay cheap.
                </p>
                {store.installedSkills.length === 0 ? (
                  <p class="text-sm text-[var(--text-secondary)]">
                    No skills installed yet.{' '}
                    <a href="/add-ons/skills" class="text-[var(--text-link)] hover:underline">Get some in Add-ons</a>.
                  </p>
                ) : (
                  <>
                    {store.installedSkills.length > 8 && (
                      <input
                        type="search"
                        value={store.skillFilter}
                        onInput$={(_, el) => { store.skillFilter = el.value; }}
                        placeholder="Find a skill"
                        class="w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm placeholder-[var(--text-muted)] border border-[var(--border-subtle)] focus:outline-none"
                      />
                    )}
                    <p class="text-xs text-[var(--text-muted)]">
                      {store.skills.length} of {store.installedSkills.length} chosen
                    </p>
                    <div class="max-h-64 overflow-y-auto space-y-1.5 pr-1">
                      {store.installedSkills
                        .filter((s) => {
                          const q = store.skillFilter.trim().toLowerCase();
                          return !q || s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q);
                        })
                        .map((s) => {
                          const on = store.skills.includes(s.name);
                          return (
                            <label key={s.name} class="flex items-start gap-2 text-sm text-[var(--text-primary)]">
                              <input
                                type="checkbox"
                                checked={on}
                                onChange$={() => {
                                  store.skills = on ? store.skills.filter((n) => n !== s.name) : [...store.skills, s.name];
                                }}
                                class="mt-0.5"
                              />
                              <span class="min-w-0">
                                <span class="font-medium">{s.name}</span>
                                <span class="text-[var(--text-muted)]"> · {tokensLabel(s.tokens)}</span>
                                {s.description && (
                                  <span class="block text-xs text-[var(--text-secondary)] line-clamp-2">{s.description}</span>
                                )}
                              </span>
                            </label>
                          );
                        })}
                    </div>
                    <a href="/add-ons/skills" class="inline-block text-sm text-[var(--text-link)] hover:underline">Get more skills</a>
                  </>
                )}
              </div>
            )}
            {(!editingAi || store.activeSection === 'appearance') && (<>
            {/* Thumbnail */}
            <div class="space-y-2">
              <p class="block text-sm font-medium text-[var(--text-secondary)]">Thumbnail</p>
              <div class="flex items-center space-x-4">
                <div class="w-16 h-16 rounded-full border border-[var(--border-subtle)] shadow-sm bg-[var(--bg-card)] flex-shrink-0 overflow-hidden flex items-center justify-center">
                  <img
                    src={sanitizeImageUrl(
                      previewSrc.value
                        || (editingAi && currentDisplayableThumbnailUrl)
                        || aiData.archetypeDefaultThumbnailsMap[store.baseArchetypeId]
                        || GENERIC_PLACEHOLDER_IMG
                    )}
                    alt="Thumbnail preview"
                    class="w-full h-full object-cover object-center"
                    onError$={(e) => {
                      const img = e.target as HTMLImageElement;
                      if (img.src.endsWith(GENERIC_PLACEHOLDER_IMG)) return;
                      img.src = GENERIC_PLACEHOLDER_IMG;
                    }}
                  />
                </div>
                <div class="flex flex-col space-y-2">
                  <button
                    type="button"
                    onClick$={() => (store.showGalleryModal = true)}
                    disabled={store.isSubmitting}
                    class={`px-4 py-2 border border-[#71717a] rounded-full text-sm font-medium text-[var(--text-secondary)] bg-[var(--bg-input)] hover:border-[#B8B0A4] hover:text-[var(--text-primary)] cursor-pointer transition-all text-left ${
                      store.isSubmitting ? 'opacity-70 cursor-not-allowed' : ''
                    }`}
                  >
                    <LuImage class="w-4 h-4 inline mr-2" />
                    Choose from Gallery
                  </button>
                  <label
                    for="thumbnailFile"
                    class={`px-4 py-2 border border-[#71717a] rounded-full text-sm font-medium text-[var(--text-secondary)] bg-[var(--bg-input)] hover:border-[#B8B0A4] hover:text-[var(--text-primary)] cursor-pointer transition-all ${
                      store.isSubmitting ? 'opacity-70 cursor-not-allowed' : ''
                    }`}
                  >
                    <LuUploadCloud class="w-4 h-4 inline mr-2" />
                    Upload Your Own
                  </label>
                  <input
                    type="file"
                    id="thumbnailFile"
                    accept="image/png, image/jpeg, image/gif, image/webp"
                    onChange$={handleFileChange$}
                    class="sr-only"
                    disabled={store.isSubmitting}
                  />
                  {(thumbnailFile.value ||
                    (editingAi &&
                      currentDisplayableThumbnailUrl &&
                      !store.useArchetypeThumbnail)) && (
                    <button
                      type="button"
                      onClick$={handleClearThumbnail$}
                      disabled={store.isSubmitting}
                      title="Remove the chosen image so the thumbnail follows the personality"
                      class={`px-4 py-2 border border-[#71717a] rounded-full text-sm font-medium text-[var(--text-secondary)] bg-[var(--bg-input)] hover:border-[#B8B0A4] hover:text-[var(--text-primary)] focus:outline-none transition-all text-left ${
                        store.isSubmitting ? 'opacity-70 cursor-not-allowed' : ''
                      }`}
                    >
                      <LuRefreshCw class="w-4 h-4 inline mr-2" /> Match Personality
                    </button>
                  )}
                </div>
              </div>
              {store.formError && thumbnailFile.value && (
                <p class="text-xs text-red-500 mt-1">
                  {store.formError.replace('Thumbnail image', 'Uploaded thumbnail image')}
                </p>
              )}
            </div>

            </>)}
            </div>
            </div>

            {/* Form Error */}
            {store.formError && (
              <div class="flex items-start p-3 bg-red-50 dark:bg-red-900/30 rounded-lg border border-red-200 dark:border-red-700/50">
                <LuAlertTriangle
                  class="h-5 w-5 text-red-500 dark:text-red-400 mr-2 flex-shrink-0"
                  aria-hidden="true"
                />
                <p class="text-sm text-red-700 dark:text-red-300">{store.formError}</p>
              </div>
            )}

            {shareStatus.value && (
              <p class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-4 py-2.5 text-xs text-[var(--text-secondary)]">
                <span class="font-medium text-[var(--text-primary)]">Shared with everyone: </span>
                {shareStatusText(shareStatus.value, editingAi?.name ?? 'It')}{' '}
                <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={$(async () => {
                  const { openUrl } = await import('@tauri-apps/plugin-opener');
                  await openUrl(shareStatus.value!.state === 'live' ? shareStatus.value!.page : shareStatus.value!.pr_url);
                })}>{shareStatus.value.state === 'live' ? 'Open the page' : 'See the submission'}</button>
              </p>
            )}
            {/* Buttons */}
            <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-3 pt-2">
              {editingAi && (
                <LiquidMetalButton
                  variant="secondary"
                  onClick$={openExport}
                  disabled={store.isSubmitting}
                  class="mt-3 sm:mt-0 sm:mr-auto w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium"
                >
                  Export AI
                </LiquidMetalButton>
              )}
              <LiquidMetalButton
                variant="secondary"
                onClick$={onClose$}
                disabled={store.isSubmitting}
                class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors disabled:opacity-70"
              >
                Cancel
              </LiquidMetalButton>
              <LiquidMetalButton
                type="submit"
                disabled={store.isSubmitting}
                class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors disabled:opacity-70"
              >
                {store.isSubmitting ? (
                  <LuLoader2 class="h-5 w-5 animate-spin mr-2" />
                ) : (
                  <LuSave class="w-[18px] h-[18px] mr-2" />
                )}
                {editingAi ? 'Save Changes' : 'Create AI'}
              </LiquidMetalButton>
            </div>
          </form>

          {/* Share dialog - the one-button path into the add-ons directory. */}
          {shareOpen.value && (
            <div class="absolute inset-0 z-20 flex items-center justify-center rounded-xl bg-black/60 p-6">
              <div class="w-full max-w-md rounded-xl bg-[var(--bg-header-footer)] p-6 shadow-2xl border border-[var(--border-subtle)]">
                <h4 class="text-base font-semibold text-[var(--text-primary)]">Share {editingAi?.name} with everyone</h4>
                {shareDone.value ? (
                  <div class="mt-3 space-y-3 text-sm text-[var(--text-secondary)]">
                    <p>
                      Submitted. {editingAi?.name} is signed with your Flowsta identity and waiting for a quick look; once it is listed it lives at{' '}
                      <span class="text-[var(--text-primary)] break-all">{shareDone.value.page}</span>
                    </p>
                    <p class="text-xs text-[var(--text-muted)]">
                      Review status:{' '}
                      <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={$(async () => {
                        const { openUrl } = await import('@tauri-apps/plugin-opener');
                        await openUrl(shareDone.value!.pr_url);
                      })}>open the submission</button>
                    </p>
                    <LiquidMetalButton variant="secondary" onClick$={$(() => { shareOpen.value = false; })} class="w-full justify-center px-5 py-2 text-sm">
                      Done
                    </LiquidMetalButton>
                  </div>
                ) : (
                  <>
                    <p class="mt-2 text-sm text-[var(--text-secondary)]">
                      Lists {editingAi?.name} on yourownai.net for everyone, as a pack: personality, portrait and Knowledge.
                      Conversations and personal memories are never included. It goes out signed with your Flowsta identity and is
                      yours to update or remove.
                    </p>
                    {!shareMakerHandle.value && (
                      <p class="mt-2 text-xs text-amber-400">
                        {exportVaultUnlocked.value
                          ? 'Sign in with Flowsta first (Settings) - a share carries your name.'
                          : exportVaultInstalled.value
                            ? 'Your Flowsta Vault is locked. Unlock it, sign in, then share.'
                            : 'Sharing needs the Flowsta Vault app and a sign-in - it is how the listing knows it is really you.'}
                      </p>
                    )}
                    <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">One line for the listing (optional)</label>
                    <input
                      type="text"
                      value={shareTitle.value}
                      onInput$={(_, el) => { shareTitle.value = el.value; }}
                      placeholder="The storyteller"
                      maxLength={60}
                      class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
                    />
                    <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">Description</label>
                    <textarea
                      value={shareDescription.value}
                      onInput$={(_, el) => { shareDescription.value = el.value; }}
                      rows={3}
                      maxLength={400}
                      class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-xl px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
                    />
                    <label class="mt-3 block text-xs font-medium text-[var(--text-secondary)]">License</label>
                    <div class="relative mt-1">
                      <button
                        type="button"
                        class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2 pl-4 pr-10 text-left text-sm text-[var(--text-primary)] border border-[var(--border-subtle)] focus:outline-none"
                        onClick$={() => { shareLicenseOpen.value = !shareLicenseOpen.value; }}
                      >
                        <span class="block truncate">{LICENSES.find((l) => l.id === shareLicense.value)?.label ?? shareLicense.value}</span>
                        <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                          <LuChevronDown class="h-4 w-4 text-[var(--text-muted)]" aria-hidden="true" />
                        </span>
                      </button>
                      {shareLicenseOpen.value && (
                        <ul class="absolute z-10 mt-1 max-h-48 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none">
                          {LICENSES.map((l) => (
                            <li
                              key={l.id}
                              class={`cursor-default select-none py-2 px-4 text-sm hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                                shareLicense.value === l.id ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)] font-medium' : 'text-[var(--text-dropdown)]'
                              }`}
                              onClick$={() => { shareLicense.value = l.id; shareLicenseOpen.value = false; }}
                            >
                              <span class="block truncate">{l.label}</span>
                            </li>
                          ))}
                        </ul>
                      )}
                    </div>
                    <p class="mt-2 text-xs text-[var(--text-muted)]">Free for everyone. Paid sharing comes later, for makers who have signed their work.</p>
                    {shareErr.value && <p class="mt-2 text-xs text-red-400">{shareErr.value}</p>}
                    <p class="mt-4 text-xs text-[var(--text-muted)]">
                      {shareMakerHandle.value
                        ? `Listed as @${shareMakerHandle.value} and signed with your Flowsta identity, so people know it is yours.`
                        : 'Sign in with Flowsta first - the listing shows who made it.'}
                    </p>
                    <div class="mt-2 flex flex-col gap-2">
                      <LiquidMetalButton
                        onClick$={doShare}
                        disabled={shareBusy.value || !shareMakerHandle.value}
                        class="w-full justify-center px-5 py-2 text-sm"
                      >
                        {shareBusy.value ? 'Signing and sending...' : 'Share'}
                      </LiquidMetalButton>
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={$(() => { shareOpen.value = false; })}
                        disabled={shareBusy.value}
                        class="w-full justify-center px-5 py-2 text-sm"
                      >
                        Cancel
                      </LiquidMetalButton>
                    </div>
                  </>
                )}
              </div>
            </div>
          )}
          {/* Export AI dialog. Two destinations side by side; signing is the
              shared footnote, and the footer says exactly what stands between
              the reader and the option they chose (nothing / a locked Vault /
              no Vault yet). */}
          {exportOpen.value && (
            <div class="absolute inset-0 z-20 flex items-center justify-center rounded-xl bg-black/60 p-6">
              <div class="w-full max-w-md rounded-xl bg-[var(--bg-header-footer)] p-6 shadow-2xl border border-[var(--border-subtle)]">
                <h4 class="text-base font-semibold text-[var(--text-primary)]">Export {editingAi?.name}</h4>
                <p class="mt-1 text-sm text-[var(--text-secondary)]">
                  A pack holds the personality, portrait and Knowledge. Conversations and personal memories stay here.
                </p>
                <div class="mt-4 grid grid-cols-2 gap-3">
                  <button
                    type="button"
                    disabled={exportBusy.value || exportChecking.value}
                    onClick$={$(async () => {
                      if (!exportVaultUnlocked.value) { exportIntent.value = 'share'; return; }
                      exportOpen.value = false; await openShare();
                    })}
                    class={`flex flex-col items-start gap-2 rounded-xl border bg-[var(--bg-card)] p-4 text-left transition-colors hover:border-[var(--text-link)] focus:outline-none focus:ring-2 focus:ring-[var(--focus-ring)] disabled:opacity-70 ${exportIntent.value === 'share' ? 'border-[var(--text-link)]' : 'border-[var(--border-subtle)]'}`}
                  >
                    <LuGlobe class="h-5 w-5 text-[var(--text-link)]" aria-hidden="true" />
                    <span class="text-sm font-semibold text-[var(--text-primary)]">{shareStatus.value?.state === 'live' ? 'Share an update' : 'Share publicly'}</span>
                    <span class="text-xs leading-snug text-[var(--text-secondary)]">Listed on yourownai.net under your Flowsta identity, for anyone to make their own.</span>
                  </button>
                  <button
                    type="button"
                    disabled={exportBusy.value || exportChecking.value}
                    onClick$={$(async () => {
                      if (!exportVaultUnlocked.value) { exportIntent.value = 'file'; return; }
                      await doExport();
                    })}
                    class={`flex flex-col items-start gap-2 rounded-xl border bg-[var(--bg-card)] p-4 text-left transition-colors hover:border-[var(--text-link)] focus:outline-none focus:ring-2 focus:ring-[var(--focus-ring)] disabled:opacity-70 ${exportIntent.value === 'file' ? 'border-[var(--text-link)]' : 'border-[var(--border-subtle)]'}`}
                  >
                    {exportBusy.value ? <LuLoader2 class="h-5 w-5 animate-spin text-[var(--text-link)]" aria-hidden="true" /> : <LuFileDown class="h-5 w-5 text-[var(--text-link)]" aria-hidden="true" />}
                    <span class="text-sm font-semibold text-[var(--text-primary)]">Export to file</span>
                    <span class="text-xs leading-snug text-[var(--text-secondary)]">A file to keep, move to another computer, or hand to a friend.</span>
                  </button>
                </div>
                {exportChecking.value ? (
                  <p class="mt-3 text-xs text-[var(--text-muted)]">Checking your Flowsta Vault...</p>
                ) : exportVaultUnlocked.value ? (
                  <p class="mt-3 text-xs text-[var(--text-muted)]">
                    Either way it carries your signature, so whoever gets {editingAi?.name} can tell it is really yours
                    {shareMakerHandle.value ? ` - signed as @${shareMakerHandle.value}.` : '.'}
                  </p>
                ) : !exportIntent.value ? (
                  <p class="mt-3 text-xs text-[var(--text-muted)]">
                    Either way it carries your signature, so whoever gets {editingAi?.name} can tell it is really yours.
                  </p>
                ) : (
                  <div class="mt-3 flex gap-3 rounded-xl border border-[var(--text-link)]/40 bg-[var(--bg-input)] p-3">
                    <LuLock class="mt-0.5 h-4 w-4 flex-shrink-0 text-[var(--text-link)]" aria-hidden="true" />
                    <div class="text-xs leading-snug text-[var(--text-secondary)]">
                      {exportVaultInstalled.value ? (
                        <>
                          <p>
                            To {exportIntent.value === 'share' ? 'share' : 'save'} {editingAi?.name} it gets your signature first.
                            The key that signs lives in your Flowsta Vault, and the Vault is locked right now.
                          </p>
                          <p class="mt-1.5">
                            Unlock it, then{' '}
                            <button type="button" class="font-medium text-[var(--text-link)] hover:underline disabled:opacity-60" disabled={exportBusy.value} onClick$={doExportSignIn}>
                              {exportBusy.value ? 'checking...' : `continue to ${exportIntent.value === 'share' ? 'sharing' : 'the file'}`}
                            </button>.
                          </p>
                        </>
                      ) : (
                        <>
                          <p>
                            To {exportIntent.value === 'share' ? 'share' : 'save'} {editingAi?.name} it gets your signature first.
                            The signing key lives in Flowsta Vault - a free app that stays on this computer and holds your Flowsta identity. Nothing about {editingAi?.name} leaves your machine to sign it.
                          </p>
                          <p class="mt-1.5">
                            <button type="button" class="font-medium text-[var(--text-link)] hover:underline" onClick$={$(async () => {
                              const { openUrl } = await import('@tauri-apps/plugin-opener');
                              await openUrl('https://flowsta.com/vault/?from=app&app=your-own-ai');
                            })}>Get Flowsta Vault</button>
                            {' '}- a few minutes, then come back here.
                          </p>
                        </>
                      )}
                    </div>
                  </div>
                )}
                {exportErr.value && (
                  <p class="mt-2 text-xs text-red-400">{exportErr.value}</p>
                )}
                <div class="mt-4 flex flex-col gap-2">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={$(() => { exportOpen.value = false; })}
                    disabled={exportBusy.value}
                    class="w-full justify-center px-5 py-2 text-sm"
                  >
                    Cancel
                  </LiquidMetalButton>
                </div>
              </div>
            </div>
          )}
          {exportNote.value && (
            <p class="absolute bottom-2 left-0 right-0 z-10 mx-auto w-fit rounded-full bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] px-4 py-1.5 text-xs text-[var(--text-secondary)]" onClick$={$(() => { exportNote.value = ''; })}>
              {exportNote.value}
            </p>
          )}
        </div>
        <ImageCropModal
          show={store.showCropModal}
          onHide$={$(() => {
            store.showCropModal = false;
          })}
          imageSrc={store.originalImageSrc}
          onCropComplete$={handleCroppedImage$}
        />
        <ThumbnailGalleryModal
          show={store.showGalleryModal}
          onHide$={$(() => {
            store.showGalleryModal = false;
          })}
          onSelect$={handleGallerySelect$}
          selectedPath={store.galleryPath}
          personalityPath={aiData.archetypeDefaultThumbnailsMap[store.baseArchetypeId] || null}
        />
      </div>
    );
  }
);

export default AiFormModal;
