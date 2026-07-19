import { component$, type Signal, type QRL, $ } from '@builder.io/qwik';
import { ChatMessage as ChatMessageComponent } from './ChatMessage';
import { Message } from '../types';

interface ConversationViewProps {
  messages: Message[];
  messagesEndRef: Signal<HTMLDivElement | undefined>;
  /** Answer an agent permission ask (folder open, by ACP request id). */
  onPermissionRespond$?: QRL<(requestId: number, decision: 'allow' | 'reject', always: boolean) => void>;
  /** The pending permission card reporting its viewport visibility. */
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
  /** Run a suggested command in the user's own terminal. */
  onOpenTerminal$?: QRL<(command: string) => void>;
  /** True while a folder-agent turn is streaming. Reserves scroll space once
   *  for the whole turn (agent bubbles skip the per-bubble reservation), so
   *  the question can anchor to the top without the view bouncing as
   *  activity cards and text segments arrive. */
  agentStreaming?: boolean;
  /** Marks the end of real content (before the turn spacer) - the
   *  follow-the-tip scroll keeps this in view while an agent turn works. */
  tipRef?: Signal<HTMLDivElement | undefined>;
  retry$: QRL<(id: string, target?: 'online' | 'device') => void>;
  canRouteOnline: boolean;
  onGround$?: QRL<(id: string) => void>;
  scrollToBottom$: QRL<(behavior?: ScrollBehavior) => void>;
  handleUpgradeClick$: QRL<() => void>;
  setSidePanelContent$: QRL<(content: { messageId: string; codeString: string; language: string; } | null) => void>;
  isDesktop: boolean;
  theme: 'light' | 'dark';
  isSidePanelVisible: boolean;
  setIsSidePanelVisible$: QRL<(isVisible: boolean) => void>;
  sidePanelContent: { messageId: string; codeString: string; language: string; } | null;
  isModelLoading: boolean;
}

export default component$<ConversationViewProps>(({
  messages,
  messagesEndRef,
  onPermissionRespond$,
  onPermissionOffscreen$,
  onOpenTerminal$,
  agentStreaming,
  tipRef,
  retry$,
  canRouteOnline,
  onGround$,
  scrollToBottom$,
  handleUpgradeClick$,
  setSidePanelContent$,
  isDesktop,
  theme,
  isSidePanelVisible,
  setIsSidePanelVisible$,
  sidePanelContent,
  isModelLoading,
}) => {
  return (
    <div class="max-w-4xl mx-auto w-full space-y-4">
      {messages.map((message, index) => (
        <ChatMessageComponent
          key={message.id}
          message={message}
          isLast={index === messages.length - 1}
          onPermissionRespond$={onPermissionRespond$}
          onPermissionOffscreen$={onPermissionOffscreen$}
          onOpenTerminal$={onOpenTerminal$}
          onRetry$={message.id ? $(() => retry$(message.id!)) : undefined}
          onRouteRetry$={message.id ? $((target: 'online' | 'device') => retry$(message.id!, target)) : undefined}
          canRouteOnline={canRouteOnline}
          onGround$={message.groundingSource && message.id ? $(() => onGround$?.(message.id!)) : undefined}
          onScrollNeeded$={scrollToBottom$}
          onUpgradeClick$={message.showUpgradeButton && message.originalUserQuery && message.id ? $(() => handleUpgradeClick$()) : undefined}
          aiLabel={message.aiLabel}
          aiImageUrl={message.aiImageUrl}
          setSidePanelContent$={setSidePanelContent$}
          isDesktop={isDesktop}
          theme={theme}
          isSidePanelVisible={isSidePanelVisible}
          setIsSidePanelVisible$={setIsSidePanelVisible$}
          sidePanelContent={sidePanelContent}
          isModelLoading={isModelLoading}
        />
      ))}
      {/* End of real content - the follow-the-tip target. */}
      <div ref={tipRef} />
      {/* Turn-scoped scroll reservation for agent turns (see agentStreaming). */}
      {agentStreaming && <div style={{ minHeight: '100vh' }} />}
      <div ref={messagesEndRef} />
    </div>
  );
});
