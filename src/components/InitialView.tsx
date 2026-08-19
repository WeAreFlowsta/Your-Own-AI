import { component$, type QRL, type Signal } from '@builder.io/qwik';
import CudaOfferCallout from './CudaOfferCallout';
import GpuFallbackCallout from './GpuFallbackCallout';
import UpdateAvailableCallout from './UpdateAvailableCallout';
import { ChatInputBar } from './ChatInputBar';
import { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';

interface InitialViewProps {
  input: Signal<string>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  currentPlaceholder: string;
  selectedAi: SelectedAiModel;
  setSelectedAi$: QRL<(ai: SelectedAiModel) => void>;
  dynamicModelOptions: SelectedAiModel[];
  getDisplayImageUrl$: QRL<(model: SelectedAiModel | undefined) => string | undefined>;
  currentSelectedOptionInListbox: SelectedAiModel | undefined;
  selectedAction: ChatAction;
  setSelectedAction$: QRL<(action: ChatAction) => void>;
  attachedFiles: Signal<AttachedFile[]>;
  attachedImages: Signal<AttachedImage[]>;
  contextWindowSize: number;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  /** Open a folder for this conversation (Build agent). */
  onOpenFolder$?: QRL<(path: string) => void>;
  /** Quiet continuity line: the last conversation, one click to re-enter. */
  lastConversationTitle?: string;
  /** The resume's visible state: opening (conductor + first read) or
   *  polling through the launch records warmup. */
  continueState?: 'idle' | 'opening' | 'warming';
  onContinueLast$?: QRL<() => void>;
  theme: 'light' | 'dark';
}

export default component$<InitialViewProps>((props) => {
  return (
    <div class="w-full max-w-4xl mx-auto">
      <ChatInputBar
        input={props.input}
        handleSubmit$={props.handleSubmit$}
        isLoading={props.isLoading}
        currentPlaceholder={props.currentPlaceholder}
        selectedAi={props.selectedAi}
        setSelectedAi$={props.setSelectedAi$}
        dynamicModelOptions={props.dynamicModelOptions}
        getDisplayImageUrl$={props.getDisplayImageUrl$}
        currentSelectedOptionInListbox={props.currentSelectedOptionInListbox}
        selectedAction={props.selectedAction}
        setSelectedAction$={props.setSelectedAction$}
        attachedFiles={props.attachedFiles}
        attachedImages={props.attachedImages}
        contextWindowSize={props.contextWindowSize}
        onAttachFiles$={props.onAttachFiles$}
        onOpenFolder$={props.onOpenFolder$}
        theme={props.theme}
      />
      {props.lastConversationTitle && props.onContinueLast$ && (
        <div class="mt-3 text-center">
          <button
            onClick$={props.onContinueLast$}
            class="inline-flex items-center gap-2 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            disabled={!!props.continueState && props.continueState !== 'idle'}
          >
            {props.continueState === 'warming' ? (
              <>
                <span class="inline-block h-3 w-3 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
                Your records are warming up - opening "{props.lastConversationTitle}" in a moment
              </>
            ) : props.continueState === 'opening' ? (
              <>
                <span class="inline-block h-3 w-3 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
                Opening "{props.lastConversationTitle}"..
              </>
            ) : (
              <>&#8635; Continue: {props.lastConversationTitle}</>
            )}
          </button>
        </div>
      )}
      <GpuFallbackCallout />
      <CudaOfferCallout />
      <UpdateAvailableCallout />
    </div>
  );
});
