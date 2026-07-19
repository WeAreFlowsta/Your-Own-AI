import { component$, useSignal, useVisibleTask$, type Signal, type QRL, $ } from '@builder.io/qwik';
import InitialView from './InitialView';
import ConversationView from './ConversationView';
import SelectionRemember from './SelectionRemember';
import { ChatInputBar } from './ChatInputBar';
import { Message, SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';

type SidePanelContent = { messageId: string; codeString: string; language: string; } | null;

interface ChatContainerProps {
  messages: Message[];
  messagesEndRef: Signal<HTMLDivElement | undefined>;
  retry$: QRL<(id: string, target?: 'online' | 'device') => void>;
  canRouteOnline: boolean;
  onGround$?: QRL<(id: string) => void>;
  scrollToBottom$: QRL<(behavior?: ScrollBehavior) => void>;
  handleUpgradeClick$: QRL<() => void>;
  input: Signal<string>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  stopChat$: QRL<() => void>;
  currentPlaceholder: string;
  selectedAi: Signal<SelectedAiModel>;
  dynamicModelOptions: SelectedAiModel[];
  currentSelectedOptionInListbox: SelectedAiModel | undefined;
  selectedAction: Signal<ChatAction>;
  sidePanelContent: Signal<SidePanelContent>;
  isDesktop: boolean;
  isSidePanelVisible: Signal<boolean>;
  theme: 'light' | 'dark';
  isModelLoading: boolean;
  // Desktop-specific props
  currentModel: string | null;
  onDownloadModel$: QRL<() => void>;
  onNewQuestion$: QRL<() => void>;
  // File context
  attachedFiles: Signal<AttachedFile[]>;
  attachedImages: Signal<AttachedImage[]>;
  contextWindowSize: number;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  // Folder (Build agent) session
  onPermissionRespond$?: QRL<(requestId: number, decision: 'allow' | 'reject', always: boolean) => void>;
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
  /** True while a permission card is pending AND scrolled out of view. */
  showPermissionPill?: boolean;
  /** True while a folder-agent turn is streaming (turn-scoped scroll space). */
  agentStreaming?: boolean;
  /** Current agent activity ("Reading config.mjs..") for the live pill. */
  liveStatus?: string;
  /** Jump to the pending permission card (pill click). */
  onPermissionJump$?: QRL<() => void>;
  /** Open a folder for this conversation (Build agent). */
  onOpenFolder$?: QRL<(path: string) => void>;
  /** Run a suggested command in the user's own terminal. */
  onOpenTerminal$?: QRL<(command: string) => void>;
}

// QRL helper: get display image URL from model (inlined, not passed as prop)
const getDisplayImageUrl$ = $((model: SelectedAiModel | undefined): string | undefined => {
  return model?.imageUrl || undefined;
});

export default component$<ChatContainerProps>((props) => {
  const { messages } = props;

  // Follow-the-tip scroll for agent turns. The view tracks the newest
  // output while the user is "attached"; scrolling up detaches instantly
  // (the tip leaving the viewport is the signal - no programmatic-scroll
  // bookkeeping needed); scrolling back until the tip is visible
  // re-attaches. Normal chat is untouched: everything gates on
  // agentStreaming.
  const tipRef = useSignal<HTMLDivElement>();
  const tipAttached = useSignal(true);

  const tipBelowFold = $((): { below: boolean; el: HTMLDivElement } | null => {
    const el = tipRef.value;
    const container = el?.closest('.overflow-y-auto') as HTMLElement | null;
    if (!el || !container) return null;
    const below =
      el.getBoundingClientRect().top > container.getBoundingClientRect().bottom + 4;
    return { below, el };
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => props.messages);
    const streaming = track(() => props.agentStreaming);
    if (!streaming) {
      tipAttached.value = true; // reset for the next turn
      return;
    }
    if (!tipAttached.value) return;
    requestAnimationFrame(async () => {
      const tip = await tipBelowFold();
      if (tip?.below) tip.el.scrollIntoView({ behavior: 'auto', block: 'end' });
    });
  });

  const handleScroll = $(async () => {
    if (!props.agentStreaming) return;
    const tip = await tipBelowFold();
    if (tip) tipAttached.value = !tip.below;
  });

  const jumpToTip = $(() => {
    tipAttached.value = true;
    tipRef.value?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  });

  // Create QRL setters from signals for child components that expect QRL callbacks
  const setSelectedAi$ = $((ai: SelectedAiModel) => { props.selectedAi.value = ai; });
  const setSelectedAction$ = $((action: ChatAction) => { props.selectedAction.value = action; });
  const setSidePanelContent$ = $((content: { messageId: string; codeString: string; language: string; } | null) => { props.sidePanelContent.value = content; });
  const setIsSidePanelVisible$ = $((v: boolean) => { props.isSidePanelVisible.value = v; });

  // Show InitialView when there are no messages
  // (WelcomeModal handles first-time setup, banner handles errors when submitting without models)
  if (messages.length === 0) {
    return (
      <div class="flex-1 flex flex-col justify-center p-4">
        <InitialView
          input={props.input}
          handleSubmit$={props.handleSubmit$}
          isLoading={props.isLoading}
          currentPlaceholder={props.currentPlaceholder}
          selectedAi={props.selectedAi.value}
          setSelectedAi$={setSelectedAi$}
          dynamicModelOptions={props.dynamicModelOptions}
          getDisplayImageUrl$={getDisplayImageUrl$}
          currentSelectedOptionInListbox={props.currentSelectedOptionInListbox}
          selectedAction={props.selectedAction.value}
          setSelectedAction$={setSelectedAction$}
          attachedFiles={props.attachedFiles}
          attachedImages={props.attachedImages}
          contextWindowSize={props.contextWindowSize}
          onAttachFiles$={props.onAttachFiles$}
          onOpenFolder$={props.onOpenFolder$}
          theme={props.theme}
        />
      </div>
    );
  }

  return (
    <div class="flex-1 flex flex-col min-h-0">
      {/* Global select-to-remember chip (one listener; no per-message chrome). */}
      <SelectionRemember />
      <div class="flex-1 overflow-y-auto p-4 scroll-smooth relative" onScroll$={handleScroll}>
        <ConversationView
          messages={props.messages}
          messagesEndRef={props.messagesEndRef}
          onPermissionRespond$={props.onPermissionRespond$}
          onPermissionOffscreen$={props.onPermissionOffscreen$}
          onOpenTerminal$={props.onOpenTerminal$}
          agentStreaming={props.agentStreaming}
          tipRef={tipRef}
          retry$={props.retry$}
          canRouteOnline={props.canRouteOnline}
          onGround$={props.onGround$}
          scrollToBottom$={props.scrollToBottom$}
          handleUpgradeClick$={props.handleUpgradeClick$}
          setSidePanelContent$={setSidePanelContent$}
          isDesktop={props.isDesktop}
          theme={props.theme}
          isSidePanelVisible={props.isSidePanelVisible.value}
          setIsSidePanelVisible$={setIsSidePanelVisible$}
          sidePanelContent={props.sidePanelContent.value}
          isModelLoading={props.isModelLoading}
        />
      </div>
      {/* Floating live pill. Permission takes precedence (the trust surface
          must never sit unseen); otherwise, while the user is scrolled away
          from a working turn's tip, it carries the current action and jumps
          back on click. */}
      {(props.showPermissionPill ||
        (props.agentStreaming && !tipAttached.value)) && (
        <div class="relative">
          <button
            onClick$={props.showPermissionPill ? props.onPermissionJump$ : jumpToTip}
            class="absolute -top-12 left-1/2 -translate-x-1/2 z-10 px-4 py-1.5 rounded-full border border-[var(--border-primary)] bg-[var(--bg-card)] text-sm text-[var(--text-primary)] shadow-lg hover:bg-[var(--bg-dropdown-hover)] whitespace-nowrap max-w-[70%] overflow-hidden text-ellipsis"
          >
            {props.showPermissionPill
              ? 'Permission needed'
              : props.liveStatus || 'Working..'}{' '}
            &darr;
          </button>
        </div>
      )}
      <div class="shrink-0 px-4 py-2 sm:py-4 border-t border-[var(--border-subtle)]">
        <ChatInputBar
          input={props.input}
          handleSubmit$={props.handleSubmit$}
          isLoading={props.isLoading}
          stopChat$={props.stopChat$}
          currentPlaceholder={props.currentPlaceholder}
          selectedAi={props.selectedAi.value}
          setSelectedAi$={setSelectedAi$}
          dynamicModelOptions={props.dynamicModelOptions}
          getDisplayImageUrl$={getDisplayImageUrl$}
          currentSelectedOptionInListbox={props.currentSelectedOptionInListbox}
          isBottomBar={true}
          selectedAction={props.selectedAction.value}
          setSelectedAction$={setSelectedAction$}
          attachedFiles={props.attachedFiles}
          attachedImages={props.attachedImages}
          contextWindowSize={props.contextWindowSize}
          onAttachFiles$={props.onAttachFiles$}
          onOpenFolder$={props.onOpenFolder$}
          theme={props.theme}
        />
      </div>
    </div>
  );
});
