import {
  component$,
  useStore,
  $,
  type QRL,
} from '@builder.io/qwik';
import { LuX, LuLink, LuDownload, LuAlertTriangle, LuLoader2, LuCheck } from '@qwikest/icons/lucide';
import { modelManager, type DownloadProgress } from '../utils/modelManager';
import { LiquidMetalBorder } from './LiquidMetalBorder';
import LiquidMetalButton from './LiquidMetalButton';

interface GGUFFile {
  name: string;
  size: number;
  url: string;
  quantization: string;
  displayName: string;
}

interface CustomModelModalProps {
  isOpen: boolean;
  onClose$: QRL<() => void>;
  onDownload$: QRL<(downloadUrl: string, filename: string) => void>;
  isDownloading?: boolean;
  downloadProgress?: DownloadProgress | null;
}

function extractQuantization(filename: string): string {
  const match = filename.match(/[_-](Q\d+_K(?:_[SML])?|Q\d+_\d+|IQ\d+_[A-Z]+|i2_s|FP16|BF16|F16|F32)/i);
  return match ? match[1].toUpperCase() : 'Unknown';
}

function extractDisplayName(filename: string): string {
  // Strip extension and common suffixes to get a cleaner name
  return filename
    .replace(/\.gguf$/i, '')
    .replace(/-\d{5}-of-\d{5}$/, ''); // multi-part suffix
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const gb = bytes / (1024 ** 3);
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = bytes / (1024 ** 2);
  return `${mb.toFixed(0)} MB`;
}

/** Sort priority: recommended quants first, then by size descending */
const QUANT_PRIORITY: Record<string, number> = {
  'Q4_K_M': 0,
  'Q4_K_S': 1,
  'Q5_K_M': 2,
  'Q5_K_S': 3,
  'Q3_K_M': 4,
  'Q3_K_L': 5,
  'Q6_K': 6,
  'Q8_0': 7,
};

const CustomModelModal = component$<CustomModelModalProps>(
  ({ isOpen, onClose$, onDownload$, isDownloading = false, downloadProgress = null }) => {
    const store = useStore({
      inputUrl: '',
      isLoading: false,
      error: null as string | null,
      ggufFiles: [] as GGUFFile[],
      selectedFile: null as GGUFFile | null,
      step: 'input' as 'input' | 'select',
    });

    if (!isOpen) return null;

    const parseHuggingFaceUrl = (url: string): { owner: string; repo: string } | null => {
      let cleanUrl = url.trim();
      cleanUrl = cleanUrl.replace(/^https?:\/\//, '');
      cleanUrl = cleanUrl.replace(/^huggingface\.co\//, '');
      cleanUrl = cleanUrl.replace(/^\/+|\/+$/g, '');
      const parts = cleanUrl.split('/');
      if (parts.length >= 2) {
        return { owner: parts[0], repo: parts[1] };
      }
      return null;
    };

    const handleClose$ = $(() => {
      store.inputUrl = '';
      store.error = null;
      store.ggufFiles = [];
      store.selectedFile = null;
      store.step = 'input';
      onClose$();
    });

    const fetchGGUFFiles$ = $(async () => {
      store.isLoading = true;
      store.error = null;

      try {
        const parsed = parseHuggingFaceUrl(store.inputUrl);

        if (!parsed) {
          store.error = 'Invalid Hugging Face URL. Please enter a valid model repository URL.';
          store.isLoading = false;
          return;
        }

        const { owner, repo } = parsed;
        const response = await fetch(`https://huggingface.co/api/models/${owner}/${repo}/tree/main`);

        if (!response.ok) {
          if (response.status === 404) {
            store.error = 'Model repository not found. Please check the URL and try again.';
          } else {
            store.error = 'Failed to fetch model files. Please try again.';
          }
          store.isLoading = false;
          return;
        }

        const data = await response.json();
        const ggufFiles: GGUFFile[] = data
          .filter((file: any) => file.type === 'file' && file.path.toLowerCase().endsWith('.gguf'))
          .map((file: any) => ({
            name: file.path,
            size: file.size || 0,
            url: `https://huggingface.co/${owner}/${repo}/resolve/main/${file.path}`,
            quantization: extractQuantization(file.path),
            displayName: extractDisplayName(file.path),
          }))
          .sort((a: GGUFFile, b: GGUFFile) => {
            const ap = QUANT_PRIORITY[a.quantization] ?? 99;
            const bp = QUANT_PRIORITY[b.quantization] ?? 99;
            if (ap !== bp) return ap - bp;
            return b.size - a.size;
          });

        if (ggufFiles.length === 0) {
          store.error = 'No GGUF files found in this repository. Make sure it contains GGUF format models.';
          store.isLoading = false;
          return;
        }

        store.ggufFiles = ggufFiles;
        store.step = 'select';

        // Auto-select the best quantization (Q4_K_M preferred)
        const q4 = ggufFiles.find(f => f.quantization === 'Q4_K_M');
        store.selectedFile = q4 || ggufFiles[0];
      } catch (err) {
        console.error('Error fetching GGUF files:', err);
        store.error = 'Failed to connect to Hugging Face. Please check your internet connection and try again.';
      } finally {
        store.isLoading = false;
      }
    });

    const handleContinue$ = $(() => {
      if (!store.inputUrl.trim()) {
        store.error = 'Please enter a Hugging Face URL';
        return;
      }
      fetchGGUFFiles$();
    });

    const handleDownload$ = $(() => {
      if (store.selectedFile) {
        onDownload$(store.selectedFile.url, store.selectedFile.name);
      }
    });

    const handleKeyPress$ = $((e: KeyboardEvent) => {
      if (e.key === 'Enter' && !store.isLoading && !isDownloading) {
        if (store.step === 'input') {
          handleContinue$();
        } else if (store.step === 'select' && store.selectedFile) {
          handleDownload$();
        }
      }
    });

    const isRecommendedQuant = (q: string) =>
      q === 'Q4_K_M' || q === 'Q5_K_M' || q === 'Q4_K_S';

    return (
      <div
        class="fixed inset-0 z-50 flex items-center justify-center p-4"
        onClick$={(e, el) => {
          // e.currentTarget is null in Qwik's async handlers — use the element arg.
          if (e.target === el) handleClose$();
        }}
        style={{
          backgroundColor: 'rgba(0, 0, 0, 0.7)',
          backdropFilter: 'blur(16px) saturate(180%)',
          WebkitBackdropFilter: 'blur(16px) saturate(180%)',
        }}
      >
        <div
          class="bg-[var(--bg-header-footer)] rounded-2xl shadow-2xl w-full max-w-2xl relative overflow-hidden"
          onClick$={(e) => e.stopPropagation()}
        >
          {/* Close button — always visible */}
          <button
            onClick$={handleClose$}
            class="absolute top-4 right-4 p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors z-10"
            aria-label="Close"
          >
            <LuX class="w-6 h-6" />
          </button>

          <div class="p-8 md:p-12">
            {/* Header */}
            <div class="mb-6">
              <h2 class="text-3xl font-bold text-[var(--text-primary)] mb-2 font-varela">
                Add Custom Model
              </h2>
              <p class="text-sm text-[var(--text-secondary)]">
                Download any GGUF model from Hugging Face
              </p>
            </div>

            {/* ── Step 1: URL Input ── */}
            {store.step === 'input' && (
              <>
                <div class="mb-6">
                  <label class="block text-sm font-medium text-[var(--text-primary)] mb-2">
                    Hugging Face URL or model ID
                  </label>
                  <LiquidMetalBorder borderRadius="0.5rem">
                    <div class="relative">
                      <LuLink class="absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] w-5 h-5" />
                      <input
                        type="text"
                        value={store.inputUrl}
                        onInput$={(e) => {
                          store.inputUrl = (e.target as HTMLInputElement).value;
                          store.error = null;
                        }}
                        onKeyPress$={handleKeyPress$}
                        placeholder="e.g. unsloth/Qwen3.5-9B-GGUF"
                        class="w-full pl-10 pr-4 py-3 bg-[var(--bg-input)] rounded-lg text-[var(--text-primary)] placeholder-[var(--text-muted)] focus:outline-none transition-all"
                        disabled={store.isLoading}
                        autoFocus
                      />
                    </div>
                  </LiquidMetalBorder>
                </div>

                {store.error && (
                  <div class="mb-6 p-4 bg-red-100 dark:bg-red-900/30 border border-red-200 dark:border-red-700 rounded-lg flex items-start gap-3">
                    <LuAlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400 flex-shrink-0 mt-0.5" />
                    <p class="text-sm text-red-700 dark:text-red-300">{store.error}</p>
                  </div>
                )}

                <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-3">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={handleClose$}
                    class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium transition-colors"
                    disabled={store.isLoading}
                  >
                    Cancel
                  </LiquidMetalButton>
                  <LiquidMetalButton
                    onClick$={handleContinue$}
                    class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium transition-colors disabled:opacity-70"
                    disabled={store.isLoading || !store.inputUrl.trim()}
                  >
                    {store.isLoading ? (
                      <>
                        <LuLoader2 class="w-[18px] h-[18px] mr-2 animate-spin" />
                        Loading...
                      </>
                    ) : (
                      'Continue'
                    )}
                  </LiquidMetalButton>
                </div>
              </>
            )}

            {/* ── Step 2: File Selection ── */}
            {store.step === 'select' && !isDownloading && (
              <>
                <p class="text-sm text-[var(--text-secondary)] mb-3">
                  {store.ggufFiles.length} variant{store.ggufFiles.length === 1 ? '' : 's'} found — pick a quantization:
                </p>

                {/* Compact file list */}
                <div class="max-h-72 overflow-y-auto mb-6 border border-[var(--border-subtle)] rounded-xl divide-y divide-[var(--border-subtle)]">
                  {store.ggufFiles.map((file) => {
                    const isSelected = store.selectedFile?.name === file.name;
                    return (
                      <button
                        key={file.name}
                        type="button"
                        onClick$={() => { store.selectedFile = file; }}
                        class={`w-full flex items-center gap-3 px-4 py-3 text-left transition-colors ${
                          isSelected
                            ? 'bg-blue-50 dark:bg-blue-900/30'
                            : 'bg-[var(--bg-card)] hover:bg-[var(--bg-dropdown-hover)]'
                        }`}
                      >
                        {/* Radio indicator */}
                        <div class={`w-5 h-5 rounded-full border-2 flex-shrink-0 flex items-center justify-center ${
                          isSelected ? 'border-blue-500 bg-blue-500' : 'border-[var(--border-subtle)]'
                        }`}>
                          {isSelected && <LuCheck class="w-3 h-3 text-white" />}
                        </div>

                        {/* File info */}
                        <div class="flex-1 min-w-0">
                          <span class="text-sm font-medium text-[var(--text-primary)] truncate block">
                            {file.displayName}
                          </span>
                        </div>

                        {/* Badges */}
                        <div class="flex items-center gap-2 flex-shrink-0">
                          {isRecommendedQuant(file.quantization) && (
                            <span class="px-1.5 py-0.5 bg-green-500 text-white text-[10px] rounded-full font-semibold">
                              Recommended
                            </span>
                          )}
                          <span class="px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] rounded text-xs text-[var(--text-secondary)] font-mono">
                            {file.quantization}
                          </span>
                          <span class="text-xs text-[var(--text-muted)] w-16 text-right">
                            {formatFileSize(file.size)}
                          </span>
                        </div>
                      </button>
                    );
                  })}
                </div>

                {/* Action buttons */}
                <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-3">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={() => {
                      store.step = 'input';
                      store.ggufFiles = [];
                      store.selectedFile = null;
                      store.error = null;
                    }}
                    class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium transition-colors"
                  >
                    Back
                  </LiquidMetalButton>
                  <LiquidMetalButton
                    onClick$={handleDownload$}
                    class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium transition-colors disabled:opacity-70"
                    disabled={!store.selectedFile}
                  >
                    <LuDownload class="w-[18px] h-[18px] mr-2" />
                    Download ({store.selectedFile ? formatFileSize(store.selectedFile.size) : ''})
                  </LiquidMetalButton>
                </div>
              </>
            )}

            {/* ── Downloading state ── */}
            {store.step === 'select' && isDownloading && (
              <div>
                <div class="bg-[var(--bg-card)] border border-[var(--border-subtle)] rounded-xl p-6 mb-6">
                  <div class="flex items-center gap-3 mb-4">
                    <LuLoader2 class="w-5 h-5 animate-spin text-blue-500 flex-shrink-0" />
                    <div class="min-w-0">
                      <p class="text-sm font-medium text-[var(--text-primary)] truncate">
                        Downloading {store.selectedFile?.displayName || store.selectedFile?.name}
                      </p>
                      <p class="text-xs text-[var(--text-muted)]">
                        {store.selectedFile?.quantization} • {store.selectedFile ? formatFileSize(store.selectedFile.size) : ''}
                      </p>
                    </div>
                  </div>

                  {downloadProgress && (
                    <>
                      <div class="w-full bg-[var(--border-subtle)] rounded-full h-2.5 overflow-hidden mb-2">
                        <div
                          class="bg-blue-600 h-full transition-all duration-300"
                          style={{ width: `${downloadProgress.percent}%` }}
                        />
                      </div>
                      <p class="text-xs text-[var(--text-muted)] text-center">
                        {downloadProgress.percent}% • {modelManager.formatModelSize(downloadProgress.downloaded)} / {modelManager.formatModelSize(downloadProgress.total)}
                      </p>
                    </>
                  )}
                </div>

                <p class="text-xs text-[var(--text-muted)] text-center mb-4">
                  You can close this dialog — the download will continue in the background.
                </p>

                <div class="flex justify-center">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={handleClose$}
                    class="inline-flex justify-center px-6 py-2.5 text-base font-medium transition-colors"
                  >
                    Close
                  </LiquidMetalButton>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }
);

export default CustomModelModal;
