import { component$, useSignal, useVisibleTask$, type QRL, type Signal } from '@builder.io/qwik';
import type { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';
import { AiSelector } from './AiSelector';
import { ContentEditor } from './ContentEditor';
import { ModelChip } from './ModelChip';
import { LiquidMetalBorder } from './LiquidMetalBorder';
import { Callout } from './Callout';
import { ONLINE_UNLOCK_TIP_ID, clearOnlineUnlockPending, onlineUnlockPending } from '../utils/entitlement';
import { isHelpDismissed } from '../utils/helpPrefs';
import type { AgentPermissionMode } from '../utils/agentPermissions';
import type { UserDefinedAI } from '../types';
import { CarryChip } from './CarryChip';

/** The one door offered for the last reply, above the composer. */
export type ReplyOffer =
  | { messageId: string; kind: 'web-off'; needsWeb: 'page' | 'live' }
  | { messageId: string; kind: 'live-info'; plan: boolean }
  | { messageId: string; kind: 'dig-deeper'; pages: number }
  | { messageId: string; kind: 'build' }
  | { messageId: string; kind: 'project-model'; text: string; bigger?: string; plan: boolean };
export type ReplyOfferTarget = 'web-once' | 'web-on' | 'web-never' | 'live-online' | 'live-use-online' | 'live-never' | 'project-online' | 'project-local' | 'project-never' | 'build-install' | 'build-dismiss' | 'build-never' | 'dig-deeper';

interface ChatInputBarProps {
  /** See ReplyOffer: at most one, for the last reply, gone on the next message. */
  replyOffer?: ReplyOffer | null;
  onReplyOffer$?: QRL<(id: string, target: ReplyOfferTarget) => void>;
  input: Signal<string>;
  /** The chip beside the model chip: what this AI carries (tools, skills),
   *  each on or off, approvals for tools, and the helper install if missing. */
  carry?: { buildInstalled: boolean; installing?: boolean; installPercent?: number; permissionMode?: AgentPermissionMode; live?: boolean };
  onCarryChanged$?: QRL<(patch: Partial<UserDefinedAI>) => void>;
  onToolsAction$?: QRL<(action: "install-projects" | "manage") => void>;
  onToolsPermission$?: QRL<(mode: AgentPermissionMode) => void>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  /** When set, decides whether the AI selector is locked (default: while loading). */
  aiSwitchLocked?: boolean;
  currentPlaceholder: string;
  /** See ContentEditor: the open conversation cannot take a message yet. */
  readOnlyReason?: string;
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
  replyOffer,
  onReplyOffer$,
  carry,
  onCarryChanged$,
  onToolsPermission$,
  onToolsAction$,
  input,
  handleSubmit$,
  isLoading,
  aiSwitchLocked,
  currentPlaceholder,
  readOnlyReason,
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
  const showCarryChip = useSignal(true);
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
    showCarryChip.value = localStorage.getItem('showChatCarryChip') !== 'false';
    // Shown once: on the Ask page, the first time after the plan activates.
    // It used to sit above every chat view for up to 14 days until Got it
    // (walkthrough 2026-10-08). Leaving the view where it showed clears it.
    let shownHere = false;
    const checkUnlock = () => {
      if (onlineUnlockPending() && isHelpDismissed(ONLINE_UNLOCK_TIP_ID)) {
        clearOnlineUnlockPending();
      }
      onlineUnlocked.value = onlineUnlockPending();
      if (onlineUnlocked.value && !isBottomBar) shownHere = true;
    };
    checkUnlock();
    window.addEventListener('entitlementChanged', checkUnlock);
    window.addEventListener('helpTipsChanged', checkUnlock);
    const onSettings = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail && 'showChatModelChip' in detail) {
        showModelChip.value = !!detail.showChatModelChip;
      }
      if (detail && 'showChatCarryChip' in detail) {
        showCarryChip.value = !!detail.showChatCarryChip;
      }
    };
    const onMemory = (e: Event) => {
      remembering.value = !!(e as CustomEvent).detail?.busy;
    };
    window.addEventListener('settingsChanged', onSettings);
    window.addEventListener('memoryExtractionChanged', onMemory);
    cleanup(() => {
      if (shownHere) clearOnlineUnlockPending();
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
  // The one offer for the last reply (see ReplyOffer) - plain consts here so
  // the click closures below capture render-scope values only.
  const offer = replyOffer ?? null;
  const offerGo = onReplyOffer$;
  const offerLink = "font-medium text-[var(--text-link)] hover:underline";
  const offerQuiet = "text-[var(--text-muted)] hover:text-[var(--text-secondary)]";

  return (
    <div class="max-w-4xl mx-auto w-full">
      {onlineUnlocked.value && !isBottomBar && (
        <Callout intent="success" title="Your AIs can now go online" id={ONLINE_UNLOCK_TIP_ID} class="mb-3">
          Change to Auto - Online and Offline to let the AI pick a model, or choose an
          online model from the {showModelChip.value ? "list at the end of the Ask row" : "model list on each card in Your AIs"}.
        </Callout>
      )}
      {offer && offerGo && (
        <Callout intent={offer.kind === 'web-off' || offer.kind === 'live-info' ? 'warning' : 'info'} class="mb-2 !p-3">
          {offer.kind === 'web-off' && (
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="web-offer">
              <span>{offer.needsWeb === 'page' ? 'Answered without reading the page.' : 'Answered without looking online.'} Web access is off for this AI.</span>
              <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'web-on')}>Turn on web access for this AI</button>
              <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'web-once')}>Just this once</button>
              <button type="button" class={offerQuiet} onClick$={() => offerGo(offer.messageId, 'web-never')}>Don't ask again for this AI</button>
            </div>
          )}
          {offer.kind === 'live-info' && (
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="live-offer" data-plan={offer.plan ? 'yes' : 'no'}>
              {offer.plan ? (
                <>
                  <span>This may be out of date. Offline models can't look up live information.</span>
                  <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'live-online')}>Look it up online</button>
                  <button type="button" class={offerLink} title="Sets this AI to Auto - Online and Offline" onClick$={() => offerGo(offer.messageId, 'live-use-online')}>Let this AI go online</button>
                </>
              ) : (
                <>
                  <span>This may be out of date. Offline models can't look up live information; online models can, as an optional paid service. Everything offline stays free.</span>
                  <a href="/online-models" class={offerLink}>See online models</a>
                </>
              )}
              <button type="button" class={offerQuiet} onClick$={() => offerGo(offer.messageId, 'live-never')}>Don't ask again for this AI</button>
            </div>
          )}
          {offer.kind === 'project-model' && (
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="project-offer" data-plan={offer.plan ? 'yes' : 'no'}>
              <span>{offer.text}</span>
              {offer.plan ? (
                <button type="button" class={offerLink} title="Sets this AI to Auto - Online and Offline and asks again" onClick$={() => offerGo(offer.messageId, 'project-online')}>Let this AI use online models</button>
              ) : (
                <a href="/online-models" class={offerLink}>See online models</a>
              )}
              {offer.bigger && (
                <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'project-local')}>Use {offer.bigger.replace(/\.gguf$/i, '').split('-').slice(0, 2).join('-')} here</button>
              )}
              <button type="button" class={offerQuiet} onClick$={() => offerGo(offer.messageId, 'project-never')}>Don't ask again for this AI</button>
            </div>
          )}
          {offer.kind === 'dig-deeper' && (
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="dig-deeper">
              <span>Looked at {offer.pages === 1 ? 'one page' : `${offer.pages} pages`}.</span>
              <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'dig-deeper')} title="Hand the same question to the Build helper, which reads on and searches when it needs to">Dig deeper</button>
            </div>
          )}
          {offer.kind === 'build' && (
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="build-offer">
              <span>One page wasn't enough here. The Build helper can research further, reading on and searching when it needs to.</span>
              <button type="button" class={offerLink} onClick$={() => offerGo(offer.messageId, 'build-install')}>Install</button>
              <button type="button" class={offerQuiet} onClick$={() => offerGo(offer.messageId, 'build-dismiss')}>Not now</button>
              <button type="button" class={offerQuiet} onClick$={() => offerGo(offer.messageId, 'build-never')}>Don't ask again</button>
            </div>
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
              isLoading={aiSwitchLocked ?? isLoading}
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
          <span class="ml-auto pl-2 inline-flex min-w-0 items-center gap-3">
            {selectedAi.aiConfig?.model && showModelChip.value && (
              <span class="hidden sm:inline-flex min-w-0">
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
            {/* What the AI carries: tools and skills, on or off. Renders
                nothing when it carries nothing. The chat holds a snapshot
                of the selected AI - patch it alongside the chip's save. */}
            {carry && showCarryChip.value && selectedAi.aiConfig && (
              <CarryChip
                ai={selectedAi.aiConfig}
                dropUp={isBottomBar}
                buildInstalled={carry.buildInstalled}
                installing={carry.installing}
                installPercent={carry.installPercent}
                permissionMode={carry.permissionMode}
                live={carry.live}
                onChanged$={(patch) => {
                  setSelectedAi$({
                    ...selectedAi,
                    aiConfig: { ...selectedAi.aiConfig, ...patch },
                  });
                  onCarryChanged$?.(patch);
                }}
                onPermission$={onToolsPermission$}
                onInstall$={() => { onToolsAction$?.("install-projects"); }}
              />
            )}
          </span>
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
            readOnlyReason={readOnlyReason}
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
