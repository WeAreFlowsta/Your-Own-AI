import { component$, useSignal, useVisibleTask$, type QRL, type Signal } from '@builder.io/qwik';
import type { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';
import { AiSelector } from './AiSelector';
import { ContentEditor } from './ContentEditor';
import { ModelChip } from './ModelChip';
import { LiquidMetalBorder } from './LiquidMetalBorder';
import { Callout } from './Callout';
import { ONLINE_UNLOCK_TIP_ID, clearOnlineUnlockPending, onlineUnlockPending } from '../utils/entitlement';
import { isHelpDismissed } from '../utils/helpPrefs';

interface ChatInputBarProps {
  input: Signal<string>;
  /** The tools line above the message field: what this AI carries, and the
   *  one thing standing in the way when there is one (Projects not installed). */
  toolsCue?: { text: string; action?: "install-projects" | "manage"; actionLabel?: string };
  onToolsAction$?: QRL<(action: "install-projects" | "manage") => void>;
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
  contextCeiling: number;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  /** Open a folder for this conversation (Build agent). */
  onOpenFolder$?: QRL<(path: string) => void>;
  theme: 'light' | 'dark';
}

export const ChatInputBar = component$<ChatInputBarProps>(({
  toolsCue,
  onToolsAction$,
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
  contextCeiling,
  onAttachFiles$,
  onOpenFolder$,
  theme,
}) => {
  // Settings > Appearance > "Model chip in chat" - default ON, an explicit
  // opt-out for people who want the Ask row bare. Reacts live to the
  // settingsChanged event so no chat reload is needed.
  const showModelChip = useSignal(true);
  // Background memory extraction after a turn: otherwise felt only as a
  // pause. A quiet hint in the Ask row while it runs.
  const remembering = useSignal(false);
  // The moment a plan activates: entitlement.ts arms a pending unlock when
  // the account goes from not-entitled to entitled; the tip sits right above
  // the Ask row because the model chip there is the answer. Cleared when
  // dismissed (Got it) or when the pending window lapses.
  const onlineUnlocked = useSignal(false);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    showModelChip.value = localStorage.getItem('showChatModelChip') !== 'false';
    const checkUnlock = () => {
      if (onlineUnlockPending() && isHelpDismissed(ONLINE_UNLOCK_TIP_ID)) {
        clearOnlineUnlockPending();
      }
      onlineUnlocked.value = onlineUnlockPending();
    };
    checkUnlock();
    window.addEventListener('entitlementChanged', checkUnlock);
    window.addEventListener('helpTipsChanged', checkUnlock);
    const onSettings = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail && 'showChatModelChip' in detail) {
        showModelChip.value = !!detail.showChatModelChip;
      }
    };
    const onMemory = (e: Event) => {
      remembering.value = !!(e as CustomEvent).detail?.busy;
    };
    window.addEventListener('settingsChanged', onSettings);
    window.addEventListener('memoryExtractionChanged', onMemory);
    cleanup(() => {
      window.removeEventListener('settingsChanged', onSettings);
      window.removeEventListener('memoryExtractionChanged', onMemory);
      window.removeEventListener('entitlementChanged', checkUnlock);
      window.removeEventListener('helpTipsChanged', checkUnlock);
    });
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
      {onlineUnlocked.value && (
        <Callout intent="success" title="Your AIs can now go online" id={ONLINE_UNLOCK_TIP_ID} class="mb-3">
          {showModelChip.value ? (
            <>
              Your plan is active. Tap the model chip at the end of the Ask row to
              give this AI an online model, or let automatic routing use both online
              and offline. The model line on every card in Your AIs switches too, and
              Settings &gt; Routing decides when a question goes online.
            </>
          ) : (
            <>
              Your plan is active. Click the model line on any card in Your AIs to
              give that AI an online model, or let automatic routing use both online
              and offline. Settings &gt; Routing decides when a question goes online.
            </>
          )}
        </Callout>
      )}
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
          {remembering.value && (
            <span
              class="ml-3 hidden sm:inline-flex items-center gap-1.5 text-[11px] text-[var(--text-muted)]"
              title="Learning durable facts from what you just said - on your device"
            >
              <span class="inline-block h-3 w-3 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
              Remembering..
            </span>
          )}
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
        {toolsCue && (
          <p class="mb-1 px-3 text-[11px] text-[var(--text-muted)]" title="Every action a tool takes goes through your approve step">
            {toolsCue.text}
            {toolsCue.action && (
              <>
                {" · "}
                <button
                  type="button"
                  class="text-[var(--text-link)] hover:underline"
                  onClick$={() => { onToolsAction$?.(toolsCue.action!); }}
                >
                  {toolsCue.actionLabel ?? (toolsCue.action === "install-projects" ? "Install" : "Manage")}
                </button>
              </>
            )}
          </p>
        )}
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
            contextCeiling={contextCeiling}
            onAttachFiles$={onAttachFiles$}
            onOpenFolder$={onOpenFolder$}
            selectedAiId={selectedAi.id}
          />
        </LiquidMetalBorder>
      </form>

    </div>
  );
});
