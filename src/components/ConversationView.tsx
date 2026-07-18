import { component$, type Signal, type QRL, $ } from '@builder.io/qwik';
import { ChatMessage as ChatMessageComponent } from './ChatMessage';
import { AgentPermissionCard } from './AgentPermissionCard';
import { AgentActivityCard } from './AgentActivityCard';
import { Message } from '../types';

interface ConversationViewProps {
  messages: Message[];
  messagesEndRef: Signal<HTMLDivElement | undefined>;
  /** Answer an agent permission card (folder open). */
  onPermissionRespond$?: QRL<(messageId: string, decision: 'allow' | 'reject', always: boolean) => void>;
  /** The pending permission card reporting its viewport visibility. */
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
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
      {messages.map((message, index) =>
        message.agentPermission ? (
          <AgentPermissionCard
            key={message.id}
            permission={message.agentPermission}
            onRespond$={
              onPermissionRespond$ &&
              $((decision: 'allow' | 'reject', always: boolean) =>
                onPermissionRespond$(message.id, decision, always))
            }
            onOffscreenChange$={onPermissionOffscreen$}
          />
        ) : message.agentRun ? (
          <AgentActivityCard key={message.id} run={message.agentRun} />
        ) : (
        <ChatMessageComponent
          key={message.id}
          message={message}
          isLast={index === messages.length - 1}
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
        ),
      )}
      <div ref={messagesEndRef} />
    </div>
  );
});
