import { component$, useSignal, useVisibleTask$, type QRL, type Signal } from '@builder.io/qwik';
import type { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';
import { AiSelector } from './AiSelector';
import { ContentEditor } from './ContentEditor';
import { ModelChip } from './ModelChip';
import { LiquidMetalBorder } from './LiquidMetalBorder';

interface ChatInputBarProps {
  input: Signal<string>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  currentPlaceholder: string;
  selectedAi: SelectedAiModel;
  setSelectedAi$: QRL<(ai: SelectedAiModel) => void>;
  dynamicModelOptions: SelectedAiModel[];
  getDisplayImageUrl$: QRL<(model: SelectedAiModel | undefined) => string | undefined>;
  currentSelectedOptionInListbox: SelectedAiModel | undefined;
  stopChat$?: QRL<() => void>;
  isBottomBar?: boolean;
  selectedAction: ChatAction;
  setSelectedAction$: QRL<(action: ChatAction) => void>;
  attachedFiles: Signal<AttachedFile[]>;
  attachedImages: Signal<AttachedImage[]>;
  contextWindowSize: number;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  /** Open a folder for this conversation (Build agent). */
  onOpenFolder$?: QRL<(path: string) => void>;
  theme: 'light' | 'dark';
}

export const ChatInputBar = component$<ChatInputBarProps>(({
  input,
  handleSubmit$,
  isLoading,
  currentPlaceholder,
  selectedAi,
  setSelectedAi$,
  dynamicModelOptions,
  getDisplayImageUrl$,
  currentSelectedOptionInListbox,
  stopChat$,
  isBottomBar = false,
  selectedAction,
  setSelectedAction$,
  attachedFiles,
  attachedImages,
  contextWindowSize,
  onAttachFiles$,
  onOpenFolder$,
  theme,
}) => {
  // Settings > Appearance > "Model chip in chat" - default ON, an explicit
  // opt-out for people who want the Ask row bare. Reacts live to the
  // settingsChanged event so no chat reload is needed.
  const showModelChip = useSignal(true);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    showModelChip.value = localStorage.getItem('showChatModelChip') !== 'false';
    const onSettings = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail && 'showChatModelChip' in detail) {
        showModelChip.value = !!detail.showChatModelChip;
      }
    };
    window.addEventListener('settingsChanged', onSettings);
    cleanup(() => window.removeEventListener('settingsChanged', onSettings));
  });

  const askBlurbText = (selectedAi.aiConfig?.askBlurb && selectedAi.aiConfig.askBlurb.trim() !== '')
    ? (() => {
        const blurb = selectedAi.aiConfig.askBlurb!;
        if (blurb.length <= 22) return blurb;
        const truncated = blurb.substring(0, 20);
        const lastSpace = truncated.lastIndexOf(' ');
        return (lastSpace > 5 ? truncated.substring(0, lastSpace) : truncated) + '..';
      })()
    : 'anything..';

  const aiSelectorPositionClass = isBottomBar ? 'absolute bottom-full mb-1' : 'absolute mt-1';

  return (
    <div class="max-w-4xl mx-auto w-full">
      <div class={isBottomBar ? 'mb-2' : 'mb-3'}>
        <div class="flex items-center relative z-10">
          <span class="mr-2 text-[var(--text-primary)] text-xs sm:text-base">Ask</span>
          <LiquidMetalBorder class="w-[165px]" borderRadius="9999px" theme={theme}>
            <AiSelector
              selectedAi={selectedAi}
              setSelectedAi$={setSelectedAi$}
              dynamicModelOptions={dynamicModelOptions}
              getDisplayImageUrl$={getDisplayImageUrl$}
              currentSelectedOptionInListbox={currentSelectedOptionInListbox}
              isLoading={isLoading}
              positionClass={aiSelectorPositionClass}
            />
          </LiquidMetalBorder>
          <span class="ml-2 text-[var(--text-primary)] text-xs sm:text-base">
            {askBlurbText}
          </span>
          {/* Quiet model chip: makes the AI's standing model choice legible
              at the point of use, one tap to change. Hidden on the smallest
              screens - the Your AIs cards cover it there. The chat holds a
              snapshot of the selected AI, so patch it alongside the
              context-level save the chip performs. */}
          {selectedAi.aiConfig?.model && showModelChip.value && (
            <span class="ml-auto pl-2 hidden sm:inline-flex min-w-0">
              <ModelChip
                aiId={selectedAi.aiConfig.id}
                model={selectedAi.aiConfig.model}
                variant="header"
                dropUp={isBottomBar}
                onChanged$={(m) => {
                  setSelectedAi$({
                    ...selectedAi,
                    aiConfig: { ...selectedAi.aiConfig, model: m },
                  });
                }}
              />
            </span>
          )}
        </div>
      </div>

      <form
        preventdefault:submit
        onSubmit$={() => { handleSubmit$(); }}
        class="flex-grow"
      >
        <LiquidMetalBorder borderRadius="1.5rem" theme={theme}>
          <ContentEditor
            input={input}
            handleSubmit$={handleSubmit$}
            isLoading={isLoading}
            currentPlaceholder={currentPlaceholder}
            selectedAction={selectedAction}
            setSelectedAction$={setSelectedAction$}
            stopChat$={stopChat$}
            attachedFiles={attachedFiles}
            attachedImages={attachedImages}
            contextWindowSize={contextWindowSize}
            onAttachFiles$={onAttachFiles$}
            onOpenFolder$={onOpenFolder$}
            selectedAiId={selectedAi.id}
          />
        </LiquidMetalBorder>
      </form>

    </div>
  );
});
