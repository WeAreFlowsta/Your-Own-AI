import {
  component$,
  useSignal,
  useTask$,
  useVisibleTask$,
  type QRL,
  type Signal,
  $,
} from '@builder.io/qwik';
import 'highlight.js/styles/vs2015.css';
import { renderMarkdown } from '../utils/renderMarkdown';
import { Message } from '../types';
import {
  LuArrowUpCircle,
  LuBot,
  LuFileText,
} from '@qwikest/icons/lucide';
import CodePanel, { SHELL_LANGUAGES, commandText } from './CodePanel';
import { AgentWorkingBox } from './AgentWorkingBox';
import ThemeAwareLottie from './ThemeAwareLottie';
import LiquidMetalButton from './LiquidMetalButton';
import type { RememberHandle } from '../utils/rememberText';

// Markdown rendering imported from shared utility (renderMarkdown)

// Inline SVG icons replacing react-icons/bs
const BsHashIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path d="M8.39 12.648a1.32 1.32 0 0 0-.015.18c0 .305.21.508.5.508.266 0 .492-.172.555-.477l.554-2.703h1.204c.421 0 .617-.234.617-.547 0-.312-.188-.53-.617-.53h-.985l.516-2.524h1.265c.43 0 .618-.227.618-.547 0-.313-.188-.524-.618-.524h-1.046l.476-2.304a1.06 1.06 0 0 0 .016-.164.51.51 0 0 0-.516-.516.54.54 0 0 0-.539.43l-.523 2.554H7.617l.477-2.304c.008-.04.015-.118.015-.164a.512.512 0 0 0-.523-.516.539.539 0 0 0-.531.43L6.53 5.484H5.414c-.43 0-.617.22-.617.532 0 .312.187.539.617.539h.906l-.515 2.523H4.609c-.421 0-.609.219-.609.531 0 .313.188.547.61.547h.976l-.516 2.492c-.008.04-.015.125-.015.18 0 .305.21.508.5.508.265 0 .492-.172.554-.477l.555-2.703h2.242l-.515 2.492zm-1-6.109h2.266l-.515 2.563H6.859l.532-2.563z" />
  </svg>
));

const BsListUlIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path
      fill-rule="evenodd"
      d="M5 11.5a.5.5 0 0 1 .5-.5h9a.5.5 0 0 1 0 1h-9a.5.5 0 0 1-.5-.5zm0-4a.5.5 0 0 1 .5-.5h9a.5.5 0 0 1 0 1h-9a.5.5 0 0 1-.5-.5zm0-4a.5.5 0 0 1 .5-.5h9a.5.5 0 0 1 0 1h-9a.5.5 0 0 1-.5-.5zm-3 1a1 1 0 1 0 0-2 1 1 0 0 0 0 2zm0 4a1 1 0 1 0 0-2 1 1 0 0 0 0 2zm0 4a1 1 0 1 0 0-2 1 1 0 0 0 0 2z"
    />
  </svg>
));

const BsLink45DegIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path d="M4.715 6.542 3.343 7.914a3 3 0 1 0 4.243 4.243l1.828-1.829A3 3 0 0 0 8.586 5.5L8 6.086a1.002 1.002 0 0 0-.154.199 2 2 0 0 1 .861 3.337L6.88 11.45a2 2 0 1 1-2.83-2.83l.793-.792a4.018 4.018 0 0 1-.128-1.287z" />
    <path d="M6.586 4.672A3 3 0 0 0 7.414 9.5l.775-.776a2 2 0 0 1-.896-3.346L9.12 3.55a2 2 0 1 1 2.83 2.83l-.793.792c.112.42.155.855.128 1.287l1.372-1.372a3 3 0 1 0-4.243-4.243L6.586 4.672z" />
  </svg>
));

const BsBookmarkIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path d="M2 2a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v13.5a.5.5 0 0 1-.777.416L8 13.101l-5.223 2.815A.5.5 0 0 1 2 15.5V2zm2-1a1 1 0 0 0-1 1v12.566l4.723-2.482a.5.5 0 0 1 .554 0L13 14.566V2a1 1 0 0 0-1-1H4z" />
  </svg>
));

const BsCpuIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path d="M5 0a.5.5 0 0 1 .5.5V2h1V.5a.5.5 0 0 1 1 0V2h1V.5a.5.5 0 0 1 1 0V2h1V.5a.5.5 0 0 1 1 0V2A2.5 2.5 0 0 1 14 4.5h1.5a.5.5 0 0 1 0 1H14v1h1.5a.5.5 0 0 1 0 1H14v1h1.5a.5.5 0 0 1 0 1H14v1h1.5a.5.5 0 0 1 0 1H14a2.5 2.5 0 0 1-2.5 2.5v1.5a.5.5 0 0 1-1 0V14h-1v1.5a.5.5 0 0 1-1 0V14h-1v1.5a.5.5 0 0 1-1 0V14h-1v1.5a.5.5 0 0 1-1 0V14A2.5 2.5 0 0 1 2 11.5H.5a.5.5 0 0 1 0-1H2v-1H.5a.5.5 0 0 1 0-1H2v-1H.5a.5.5 0 0 1 0-1H2v-1H.5a.5.5 0 0 1 0-1H2A2.5 2.5 0 0 1 4.5 2V.5A.5.5 0 0 1 5 0zm-.5 3A1.5 1.5 0 0 0 3 4.5v7A1.5 1.5 0 0 0 4.5 13h7a1.5 1.5 0 0 0 1.5-1.5v-7A1.5 1.5 0 0 0 11.5 3h-7zM5 6.5A1.5 1.5 0 0 1 6.5 5h3A1.5 1.5 0 0 1 11 6.5v3A1.5 1.5 0 0 1 9.5 11h-3A1.5 1.5 0 0 1 5 9.5v-3zM6.5 6a.5.5 0 0 0-.5.5v3a.5.5 0 0 0 .5.5h3a.5.5 0 0 0 .5-.5v-3a.5.5 0 0 0-.5-.5h-3z" />
  </svg>
));

const BsTerminalIcon = component$(() => (
  <svg
    fill="currentColor"
    stroke="currentColor"
    stroke-width="0"
    viewBox="0 0 16 16"
    height="1em"
    width="1em"
    class="md:mr-1.5"
  >
    <path d="M6 9a.5.5 0 0 1 .5-.5h3a.5.5 0 0 1 0 1h-3A.5.5 0 0 1 6 9zM3.854 4.146a.5.5 0 1 0-.708.708L4.793 6.5 3.146 8.146a.5.5 0 1 0 .708.708l2-2a.5.5 0 0 0 0-.708l-2-2z" />
    <path d="M2 1a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V3a2 2 0 0 0-2-2H2zm12 1a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1h12z" />
  </svg>
));

interface ChatMessageProps {
  message: Message;
  isLast?: boolean;
  onRetry$?: QRL<(originalContent: string | undefined) => void>;
  onGround$?: QRL<() => void>;
  onScrollNeeded$?: QRL<() => void>;
  onUpgradeClick$?: QRL<(originalQuery: string, triggerMessageId: string) => void>;
  /** Redo this turn on the other side ("Try online" / "Redo on your device"). */
  onRouteRetry$?: QRL<(target: 'online' | 'device') => void>;
  /** Whether this AI may go online at all (auto online-offline mode). */
  canRouteOnline?: boolean;
  aiLabel?: string;
  aiImageUrl?: string | null;
  setSidePanelContent$: QRL<(content: { messageId: string; codeString: string; language: string } | null) => void>;
  /** Answer a permission ask inside this message's agent working log. */
  onPermissionRespond$?: QRL<(requestId: number, decision: 'allow' | 'reject', always: boolean) => void>;
  /** The pending permission card reporting its viewport visibility. */
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
  /** Run a suggested command in the user's own terminal. */
  onOpenTerminal$?: QRL<(command: string) => void>;
  /** Undo the file changes of this message's agent turn (last finished turn only). */
  onUndoTurn$?: QRL<(messageId: string) => void>;
  /** Live retry text for the active agent turn's pearl. */
  agentRetryStatus?: string;
  /** Online model the agent's current call is waiting on (bare id), if any. */
  agentWaitingOn?: string;
  isDesktop: boolean;
  theme: 'light' | 'dark';
  isSidePanelVisible: boolean;
  setIsSidePanelVisible$: QRL<(isVisible: boolean) => void>;
  sidePanelContent: { messageId: string; codeString: string; language: string } | null;
  isModelLoading: boolean;
}

/** Short human label + origin for the routing receipt strip. */
function servedByParts(servedBy: string): { name: string; origin: string } {
  if (servedBy.startsWith('online:')) return { name: servedBy.slice(7), origin: 'online' };
  if (servedBy.startsWith('external:')) return { name: servedBy.slice(9), origin: 'your server' };
  return { name: servedBy.replace(/\.gguf$/i, ''), origin: 'on device' };
}

// Helper function to process citation markers in an HTML string
function processCitationMarkersHtml(html: string): string {
  return html.replace(
    /\[(\d+)\]/g,
    '<sup class="text-[8px] font-medium relative top-[-4px] mx-0.5">$1</sup>'
  );
}

// Local renderMarkdown wrapper that also processes citation markers
function renderMarkdownWithCitations(text: string): string {
  let html = renderMarkdown(text);
  html = processCitationMarkersHtml(html);
  return html;
}

// Short code reads best in place; long code reads best in the panel. So a
// block up to this many lines stays fully inline (no lift, no chip), and the
// first block beyond it is lifted into the code panel - replaced IN PLACE by
// the token below, which renders as a preview of its first lines fading into
// a "Show all N lines" chip. The code is visibly THERE either way: a reply
// can never look like it lost its code (people told the AI it had produced
// no code when it was one click away).
const INLINE_CODE_MAX_LINES = 14;
const CODE_PREVIEW_LINES = 8;
const CODE_ANCHOR_TOKEN = 'YOAICODEANCHOR7f3a';

// Helper function to parse code blocks from markdown
const parseCodeFromMarkdown = (
  markdown: string
): { mainText: string; codeString: string; language: string; lineCount: number } | null => {
  const codeBlockRegex = /```(\w+)?\n([\s\S]+?)\n```/g;
  let match: RegExpExecArray | null;
  while ((match = codeBlockRegex.exec(markdown)) !== null) {
    const codeString = match[2].trim();
    const lineCount = codeString.split('\n').length;
    if (lineCount <= INLINE_CODE_MAX_LINES) continue; // short code stays in the prose
    const mainText = (
      markdown.slice(0, match.index) +
      `\n\n${CODE_ANCHOR_TOKEN}\n\n` +
      markdown.slice(match.index + match[0].length)
    ).trim();
    return { mainText, codeString, language: match[1] || 'plaintext', lineCount };
  }
  return null;
};

// Streaming mirror of parseCodeFromMarkdown: short complete blocks stay
// inline as they finish, the first long complete block becomes the anchor
// (it is already in the panel), later long blocks and the still-open tail
// are held back until the reply is done.
const stripIncompleteCodeBlocks = (text: string): string => {
  const re = /```(\w+)?\n([\s\S]+?)\n```/g;
  let out = '';
  let last = 0;
  let anchored = false;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    out += text.slice(last, m.index);
    const lineCount = m[2].trim().split('\n').length;
    if (lineCount <= INLINE_CODE_MAX_LINES) {
      out += m[0];
    } else if (!anchored) {
      anchored = true;
      out += `\n\n${CODE_ANCHOR_TOKEN}\n\n`;
    }
    last = m.index + m[0].length;
  }
  // Only the tail (after the last complete block) can hold an open fence.
  out += text.slice(last).replace(/```[\s\S]*$/, '');
  return out.trim();
};

const escapeHtml = (s: string): string =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** The inline chip that stands where the extracted code block was. Clicks are
 *  handled by delegation on the message body (the chip lives in rendered HTML). */
const codeAnchorHtml = (language: string, lineCount: number, open: boolean, preview = false): string => {
  const label = preview
    ? `${open ? 'Hide code' : `Show all ${lineCount} lines`} · ${escapeHtml(language)}`
    : `${escapeHtml(language)} · ${lineCount} line${lineCount === 1 ? '' : 's'} · ${open ? 'Hide code' : 'Show code'}`;
  return (
    '<button type="button" data-code-anchor="1" class="code-anchor inline-flex items-center gap-1.5 my-1 px-2.5 py-1 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-dropdown)] text-xs font-medium text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:border-[var(--text-muted)] transition-colors cursor-pointer">' +
    '<svg fill="currentColor" viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M6 9a.5.5 0 0 1 .5-.5h3a.5.5 0 0 1 0 1h-3A.5.5 0 0 1 6 9zM3.854 4.146a.5.5 0 1 0-.708.708L4.793 6.5 3.146 8.146a.5.5 0 1 0 .708.708l2-2a.5.5 0 0 0 0-.708l-2-2z"/><path d="M2 1a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V3a2 2 0 0 0-2-2H2zm12 1a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1h12z"/></svg>' +
    `<span>${label}</span></button>`
  );
};

/** The inline stand-in for a lifted long block: its first lines rendered
 *  exactly like any code block, fading out, with the chip as its footer. */
const codePreviewHtml = (
  codeData: { codeString: string; language: string; lineCount: number },
  open: boolean
): string => {
  const head = codeData.codeString.split('\n').slice(0, CODE_PREVIEW_LINES).join('\n');
  const fence =
    '```' + (codeData.language === 'plaintext' ? '' : codeData.language) + '\n' + head + '\n```';
  const fade =
    'mask-image:linear-gradient(to bottom,black 45%,transparent 100%);' +
    '-webkit-mask-image:linear-gradient(to bottom,black 45%,transparent 100%)';
  return (
    '<div class="code-preview" data-code-anchor-preview="1">' +
    `<div style="max-height:11rem;overflow:hidden;${fade}" aria-hidden="true">${renderMarkdown(fence)}</div>` +
    codeAnchorHtml(codeData.language, codeData.lineCount, open, true) +
    '</div>'
  );
};

/** Swap the anchor token for the preview (or chip) in rendered HTML. The
 *  token renders as its own paragraph when the block stood alone; the
 *  stand-in takes the paragraph's place (block content cannot live in a p). */
const placeCodeAnchor = (html: string, standin: string): string =>
  html.replace(new RegExp(`<p>\\s*${CODE_ANCHOR_TOKEN}\\s*</p>`, 'g'), standin).replace(new RegExp(CODE_ANCHOR_TOKEN, 'g'), standin);

// Helper function to check if we're currently inside an incomplete code block
const isInsideIncompleteCodeBlock = (text: string): boolean => {
  const openingMatches = text.match(/```/g);
  if (!openingMatches) return false;
  return openingMatches.length % 2 !== 0;
};

// ActionBar as a separate component$
interface ActionBarProps {
  message: Message;
  isLoading: boolean;
  isModelLoading: boolean;
  isWritingCode: boolean;
  isThinking: boolean;
  isGenerationPhase: boolean;
  statusText?: string;
  dynamicStatus?: string;
  statusOpacity?: number;
  aiName: string;
  theme: 'light' | 'dark';
  onGround$?: QRL<() => void>;
  isLast?: boolean;
  onRouteRetry$?: QRL<(target: 'online' | 'device') => void>;
  canRouteOnline?: boolean;
  /** Agent turns: the work rail's expansion state - the Steps button and
   *  the collapsed stub toggle the same signal. */
  railOpen?: Signal<boolean>;
}

// Minimum reasoning length (trimmed chars) worth surfacing a thinking box for.
// Some models emit only a one-line restatement of the question as "reasoning"
// (notably Grok's web-search summary path, which exposes a condensed summary
// rather than the full reasoning) — a thinking box for that adds nothing. This
// is a uniform gate across all models, not a per-model special case.
const MIN_THINKING_DISPLAY_CHARS = 100;

const ActionBar = component$<ActionBarProps>((props) => {
  const openSection = useSignal<'tokens' | 'thoughts' | 'sources' | 'model' | null>(null);
  /** '' | 'saving' | 'saved' | 'error' - the Remember button's state. Saved is
   *  a TOGGLE: clicking again forgets the save (via the kept handle). */
  const rememberState = useSignal('');
  const rememberHandle = useSignal<RememberHandle | null>(null);

  // Reflect an already-saved reply across reloads (cheap: one index per store,
  // shared by every button on the page).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => props.isLoading);
    if (props.isLoading || props.message.role !== 'assistant' || !props.message.content) return;
    try {
      const { findRememberedCached } = await import('../utils/rememberText');
      const h = await findRememberedCached(props.message.model, props.message.content, 'reply');
      if (h) {
        rememberHandle.value = h;
        rememberState.value = 'saved';
      }
    } catch {
      /* display-only check */
    }
  });

  const rememberReply = $(async () => {
    if (rememberState.value === 'saving') return;
    if (rememberState.value === 'saved' && rememberHandle.value) {
      rememberState.value = 'saving';
      try {
        const { forgetRemembered } = await import('../utils/rememberText');
        await forgetRemembered(props.message.model, rememberHandle.value);
        rememberHandle.value = null;
        rememberState.value = '';
      } catch {
        rememberState.value = 'saved';
      }
      return;
    }
    rememberState.value = 'saving';
    try {
      const { rememberText } = await import('../utils/rememberText');
      const handle = await rememberText(props.message.model, props.message.content, 'reply');
      rememberHandle.value = handle;
      rememberState.value = handle ? 'saved' : 'error';
    } catch {
      rememberState.value = 'error';
    }
    if (rememberState.value === 'error') {
      setTimeout(() => (rememberState.value = ''), 2500);
    }
  });

  // Folder sessions have TWO honest destinations for a remembered reply:
  // the folder's shared memory (likelier mid-work) or this AI's own memory.
  // The little menu makes the choice explicit - never a silent default.
  const rememberMenuOpen = useSignal(false);
  const rememberFolder = useSignal<string | null>(null);

  const rememberClick = $(async () => {
    if (rememberState.value === 'saving') return;
    // Saved-toggle (per-AI) keeps its forget-on-click behavior.
    if (rememberState.value === 'saved' && rememberHandle.value) {
      await rememberReply();
      return;
    }
    if (props.message.agentTurn) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const status = await invoke<{ folder: string | null }>('build_agent_status');
        if (status.folder) {
          rememberFolder.value = status.folder;
          rememberMenuOpen.value = true;
          return;
        }
      } catch {
        /* no bridge - plain per-AI save */
      }
    }
    await rememberReply();
  });

  const rememberToFolder = $(async () => {
    const folder = rememberFolder.value;
    rememberMenuOpen.value = false;
    if (!folder) return;
    rememberState.value = 'saving';
    try {
      const { appendWorkspaceNote } = await import('../utils/workspaceMemory');
      const ok = await appendWorkspaceNote(folder, props.message.content);
      rememberState.value = ok ? 'saved-folder' : 'error';
    } catch {
      rememberState.value = 'error';
    }
    setTimeout(() => {
      if (rememberState.value === 'saved-folder' || rememberState.value === 'error') {
        rememberState.value = '';
      }
    }, 2500);
  });

  const hasTokens = props.message.tokens && (props.message.tokens.total_tokens || 0) > 0;
  const hasThoughts = !!props.message.thinking && props.message.thinking.trim().length >= MIN_THINKING_DISPLAY_CHARS;
  const hasSources = props.message.sources && props.message.sources.length > 0;
  const hasGrounded = !!props.message.grounded && props.message.grounded.length > 0;
  const canGround = !!props.onGround$ && !hasGrounded && !props.message.groundingPending;
  const hasModelInfo = props.message.role === 'assistant' && !!props.message.servedBy;
  const hasSteps =
    !!props.railOpen && !!props.message.agentLog && props.message.agentLog.length > 0 && !props.message.isLoading;
  const hasButtons = hasSteps || hasTokens || hasThoughts || hasSources || hasGrounded || canGround || !!props.message.groundingPending || !!props.message.groundingNote || hasModelInfo;
  const showStatus = props.isLoading && !props.message.error;

  // Nothing to say and nothing to offer: render nothing. (The empty bar
  // container was the "dead strip" above agent turns.)
  if (!showStatus && !hasButtons) {
    return null;
  }

  const toggleSection$ = $((section: 'tokens' | 'thoughts' | 'sources' | 'model') => {
    openSection.value = openSection.value === section ? null : section;
  });

  // Determine status animation type and text.
  // TURN-scoped on purpose: the header widget owns global loading state; the
  // bubble only reports a load THIS turn is waiting on (message.statusText is
  // set exactly then and cleared on the first streamed token). Keying off the
  // global isModelLoading here made background warm-loads (startup
  // page-ensure) badge unrelated replies with a textless loading icon.
  // A search turn's statusText is live search progress, not a model load —
  // it renders with the thinking animation, not the loading one.
  const isShowingModelLoading = !!props.statusText && !props.message.searchingWeb;

  // A model load can take tens of seconds. After a moment, a slow rotation
  // of short lines explains what the wait IS (the model moving into
  // memory) and why it won't repeat (it stays loaded) - then holds on the
  // last line. Fast loads never show any of it, so pros see nothing extra.
  const loadingHintIdx = useSignal(-1);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const active = track(() => isShowingModelLoading && !!props.message.isLoading);
    loadingHintIdx.value = -1;
    if (!active) return;
    const timers: ReturnType<typeof setTimeout>[] = [];
    timers.push(setTimeout(() => (loadingHintIdx.value = 0), 3000));
    timers.push(setTimeout(() => (loadingHintIdx.value = 1), 8000));
    timers.push(setTimeout(() => (loadingHintIdx.value = 2), 13000));
    cleanup(() => timers.forEach(clearTimeout));
  });
  const LOADING_HINTS = [
    'moving the model into memory, a one-time step',
    'bigger models take longer to wake up',
    'once loaded it stays ready - your next questions answer right away',
  ];

  // Nothing is loading, but no words yet: after a few seconds say what the
  // wait IS (the model reading the question and its instructions - long on
  // a slow engine, once) so silence never reads as a hang. Chat turns only.
  const readingHintIdx = useSignal(-1);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const active = track(
      () =>
        !isShowingModelLoading &&
        !!props.message.isLoading &&
        !props.message.content &&
        !props.message.agentTurn &&
        !props.message.searchingWeb,
    );
    readingHintIdx.value = -1;
    if (!active) return;
    const timers: ReturnType<typeof setTimeout>[] = [];
    timers.push(setTimeout(() => (readingHintIdx.value = 0), 5000));
    timers.push(setTimeout(() => (readingHintIdx.value = 1), 15000));
    cleanup(() => timers.forEach(clearTimeout));
  });
  const READING_HINTS = [
    'reading your question and its instructions',
    'a long first read on this engine - the next questions reuse it',
  ];

  const statusType = isShowingModelLoading
    ? 'loading' as const
    : props.isWritingCode
      ? 'coding' as const
      : 'thinking' as const;

  // A web-search model can research for 30-60s before its first visible
  // text - name that phase honestly rather than showing a generic
  // "thinking" that reads as a hang.
  const statusText = props.statusText
    ? props.statusText
    : props.isWritingCode
      ? 'Writing code..'
      : props.message.searchingWeb && !props.message.content
        ? 'Searching the web..'
        : props.message.agentTurn
          ? `${props.aiName} is working..`
          : `${props.aiName} is thinking..`;

  return (
    <div class="generic-container my-2 rounded-2xl pl-9 md:pl-10 lg:pl-10">
      <div class="text-sm text-[var(--text-secondary)] px-2 py-1 w-full flex items-center rounded-2xl min-h-[30px]">
        {/* Status indicator (left-aligned, shown during loading) */}
        {showStatus && (
          <div class="flex items-center gap-1 md:gap-1.5 flex-shrink-0 italic">
            <div class="block md:hidden translate-y-[1px]">
              <ThemeAwareLottie type={statusType} theme={props.theme} size={14} />
            </div>
            <div class="hidden md:block translate-y-[2px]">
              <ThemeAwareLottie type={statusType} theme={props.theme} size={18} />
            </div>
            <span class="animate-pulse-text status-text-gradient text-[10px] leading-tight md:text-xs">
              {statusText}
              {props.isGenerationPhase && props.dynamicStatus && (
                <em
                  class="italic block md:inline md:ml-3"
                  style={{ opacity: props.statusOpacity ?? 1, transition: 'opacity 300ms ease-in-out' }}
                >
                  {props.dynamicStatus}
                </em>
              )}
              {!isShowingModelLoading && readingHintIdx.value >= 0 && !props.message.content && (
                <em
                  key={`r${readingHintIdx.value}`}
                  class="italic block md:inline md:ml-3 opacity-70 animate-fade-in"
                >
                  {READING_HINTS[Math.min(readingHintIdx.value, READING_HINTS.length - 1)]}
                </em>
              )}
              {isShowingModelLoading && loadingHintIdx.value >= 0 && (
                <em
                  key={loadingHintIdx.value}
                  class="italic block md:inline md:ml-3 opacity-70 animate-fade-in"
                >
                  {LOADING_HINTS[Math.min(loadingHintIdx.value, LOADING_HINTS.length - 1)]}
                </em>
              )}
            </span>
          </div>
        )}

        {/* Spacer to push buttons right */}
        <div class="flex-grow" />

        {/* Action buttons (right-aligned, shown when data available) */}
        {hasButtons && !showStatus && (
          <>
          <div class="flex items-center space-x-2">
            {hasSteps && (
              <LiquidMetalButton
                onClick$={() => {
                  props.railOpen!.value = !props.railOpen!.value;
                }}
                class="px-3 py-1 text-xs flex items-center"
                aria-expanded={props.railOpen!.value}
                title="The work behind this answer - every step, in order"
              >
                <BsListUlIcon />
                <span class="hidden md:inline">
                  {props.railOpen!.value ? 'Hide Steps' : 'Steps'}
                </span>
              </LiquidMetalButton>
            )}
            {hasTokens && (
              <LiquidMetalButton
                onClick$={() => toggleSection$('tokens')}
                class="px-3 py-1 text-xs flex items-center"
                aria-expanded={openSection.value === 'tokens'}
              >
                <BsHashIcon />
                <span class="hidden md:inline">Tokens</span>
              </LiquidMetalButton>
            )}
            {props.message.role === 'assistant' && !!props.message.content && (
              <span class="relative inline-flex">
                <LiquidMetalButton
                  onClick$={rememberClick}
                  class="px-3 py-1 text-xs flex items-center"
                  title={
                    rememberState.value === 'error'
                      ? 'Could not save - the memory model may still be downloading (Settings - Components)'
                      : rememberState.value === 'saved'
                        ? 'Remembered - click to forget it again'
                        : rememberState.value === 'saved-folder'
                          ? 'Saved to this project’s memory'
                          : 'Remember this reply - your AI will draw on it in future conversations'
                  }
                  disabled={rememberState.value === 'saving'}
                >
                  <BsBookmarkIcon />
                  <span class="hidden md:inline">
                    {rememberState.value === 'saved'
                      ? 'Remembered'
                      : rememberState.value === 'saved-folder'
                        ? 'Saved to project'
                        : rememberState.value === 'saving'
                          ? 'Saving...'
                          : rememberState.value === 'error'
                            ? 'Try again'
                            : 'Remember'}
                  </span>
                </LiquidMetalButton>
                {rememberMenuOpen.value && (
                  <>
                    <span
                      class="fixed inset-0 z-[45]"
                      onClick$={() => (rememberMenuOpen.value = false)}
                    />
                    <span class="absolute left-0 bottom-full mb-2 w-56 rounded-2xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] py-1 shadow-lg z-50 overflow-hidden block">
                      <button
                        type="button"
                        onClick$={rememberToFolder}
                        class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] text-left"
                      >
                        Remember for this project
                      </button>
                      <button
                        type="button"
                        onClick$={() => {
                          rememberMenuOpen.value = false;
                          rememberReply();
                        }}
                        class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] text-left"
                      >
                        Remember for {props.message.aiLabel || 'this AI'}
                      </button>
                    </span>
                  </>
                )}
              </span>
            )}
            {hasModelInfo && (
              <LiquidMetalButton
                onClick$={() => toggleSection$('model')}
                class="px-3 py-1 text-xs flex items-center"
                aria-expanded={openSection.value === 'model'}
                aria-label="Which model answered this and why"
              >
                <BsCpuIcon />
                <span class="hidden md:inline">Model</span>
              </LiquidMetalButton>
            )}
            {hasThoughts && (
              <LiquidMetalButton
                onClick$={() => toggleSection$('thoughts')}
                class="px-3 py-1 text-xs flex items-center"
                aria-expanded={openSection.value === 'thoughts'}
              >
                <BsTerminalIcon />
                <span class="hidden md:inline">
                  {openSection.value === 'thoughts' ? 'Hide Thoughts' : 'Show Thoughts'}
                </span>
              </LiquidMetalButton>
            )}
            {(hasSources || hasGrounded || !!props.message.groundingNote) && (
              <LiquidMetalButton
                onClick$={() => toggleSection$('sources')}
                class="px-3 py-1 text-xs flex items-center"
                aria-expanded={openSection.value === 'sources'}
              >
                <BsLink45DegIcon />
                <span class="hidden md:inline">Sources</span>
              </LiquidMetalButton>
            )}
            {props.message.groundingPending && !hasGrounded && (
              <span class="px-3 py-1 text-xs flex items-center gap-1.5 text-[var(--text-muted)]">
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--text-muted)] animate-pulse" />
                <span class="hidden md:inline">Verifying sources…</span>
              </span>
            )}
            {canGround && (
              <LiquidMetalButton
                onClick$={() => { openSection.value = 'sources'; props.onGround$!(); }}
                class="px-3 py-1 text-xs flex items-center"
                aria-label="Check this answer's claims against the attached document"
              >
                <BsLink45DegIcon />
                <span class="hidden md:inline">Verify sources</span>
              </LiquidMetalButton>
            )}
          </div>
          </>
        )}
      </div>

      {openSection.value === 'model' && hasModelInfo && (
        <div class="px-4 pb-4 -mt-2">
          <div class="pt-4 text-sm text-[var(--text-secondary)]">
            <span class="font-medium text-[var(--text-primary)]">
              {servedByParts(props.message.servedBy!).name}
            </span>
            {' · '}
            {servedByParts(props.message.servedBy!).origin}
            <p class="text-xs mt-1.5">
              {props.message.routingReason
                ? `Auto routing: ${props.message.routingReason}`
                : 'This model was picked directly, not by Auto routing.'}
            </p>
            {!!props.message.skills?.length && (
              <p class="text-xs mt-1.5">
                Skills carried on this turn: {props.message.skills.join(', ')}
              </p>
            )}
            {props.message.agentTurn && (
              <p class="text-xs mt-1.5 text-[var(--text-muted)]">
                A folder session stays on its model for the whole conversation.
              </p>
            )}
            {props.isLast &&
              props.onRouteRetry$ &&
              !props.message.agentTurn &&
              props.message.servedBy!.startsWith('online:') && (
                <button
                  type="button"
                  onClick$={() => {
                    openSection.value = null;
                    props.onRouteRetry$!('device');
                  }}
                  class="mt-2.5 px-3 py-1.5 text-xs rounded-lg bg-[var(--bg-button-secondary)] hover:bg-[var(--bg-button-secondary-hover)] border border-[var(--border-secondary)] text-[var(--text-button-secondary)] transition-colors"
                >
                  Redo on your device
                </button>
              )}
            {props.isLast &&
              props.onRouteRetry$ &&
              !props.message.agentTurn &&
              props.canRouteOnline &&
              !props.message.servedBy!.startsWith('online:') && (
                <button
                  type="button"
                  onClick$={() => {
                    openSection.value = null;
                    props.onRouteRetry$!('online');
                  }}
                  class="mt-2.5 px-3 py-1.5 text-xs rounded-lg bg-[var(--bg-button-secondary)] hover:bg-[var(--bg-button-secondary-hover)] border border-[var(--border-secondary)] text-[var(--text-button-secondary)] transition-colors"
                >
                  Try this answer online
                </button>
              )}
          </div>
        </div>
      )}

      {openSection.value === 'tokens' && hasTokens && (
        <div class="px-4 pb-4 -mt-2 text-[var(--text-primary)] whitespace-pre-wrap text-base leading-relaxed tracking-normal font-light">
          <div class={`pt-4 grid ${props.message.tokens?.tokens_per_second ? (props.message.tokens?.prompt_per_second ? 'grid-cols-5' : 'grid-cols-4') : 'grid-cols-3'} gap-2 text-center`}>
            <div>
              <div class="font-semibold text-base">{props.message.tokens?.prompt_tokens}</div>
              <div class="text-xs text-[var(--text-muted)]">Prompt</div>
            </div>
            <div>
              <div class="font-semibold text-base">{props.message.tokens?.completion_tokens}</div>
              <div class="text-xs text-[var(--text-muted)]">Completion</div>
            </div>
            <div>
              <div class="font-semibold text-base">{props.message.tokens?.total_tokens}</div>
              <div class="text-xs text-[var(--text-muted)]">Total</div>
            </div>
            {props.message.tokens?.prompt_per_second && (
              <div>
                <div class="font-semibold text-base">{props.message.tokens.prompt_per_second}</div>
                <div class="text-xs text-[var(--text-muted)]">Reading tok/s</div>
              </div>
            )}
            {props.message.tokens?.tokens_per_second && (
              <div title="Completion tokens over the time they streamed - the same measure for every engine">
                <div class="font-semibold text-base">{props.message.tokens.tokens_per_second}</div>
                <div class="text-xs text-[var(--text-muted)]">
                  Writing tok/s{props.message.tokens.engine === 'mlx' ? ' · MLX' : ''}
                </div>
              </div>
            )}
          </div>
          {/* Agent turns are many model calls over real time - say so. */}
          {props.message.agentStats && (
            <div class="mt-3 text-center text-xs text-[var(--text-muted)]">
              {props.message.agentStats.modelCalls ?? '?'} model call
              {(props.message.agentStats.modelCalls ?? 0) === 1 ? '' : 's'}
              {props.message.agentStats.durationMs
                ? ` · ${Math.round(props.message.agentStats.durationMs / 1000)}s of model time`
                : ''}
            </div>
          )}
        </div>
      )}

      {openSection.value === 'thoughts' && hasThoughts && (
        // No whitespace-pre-wrap here: the markdown already turns blank lines
        // into paragraphs, and pre-wrap preserved the source newlines ON TOP
        // of the paragraph margins - every gap doubled.
        <div
          class="px-4 pb-4 -mt-2 text-[var(--text-secondary)] text-xs leading-relaxed font-light opacity-80"
          role="region"
          aria-live="polite"
        >
          <div class="pt-4 markdown-content thinking-markdown" dangerouslySetInnerHTML={renderMarkdown(props.message.thinking || '')} />
        </div>
      )}

      {openSection.value === 'sources' &&
        (hasSources || hasGrounded || props.message.groundingPending || props.message.groundingNote) && (
        <div class="px-4 pb-4 -mt-2 text-[var(--text-primary)] text-base leading-relaxed tracking-normal font-light">
          <div class="pt-4 space-y-4">
            {/* Verify-sources outcome lives here, where located quotes would
                appear - not in the button strip. */}
            {props.message.groundingPending && !hasGrounded && (
              <div class="text-xs text-[var(--text-muted)] flex items-center gap-1.5">
                <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--text-muted)] animate-pulse" />
                Verifying sources…
              </div>
            )}
            {!props.message.groundingPending && !hasGrounded && props.message.groundingNote && (
              <div>
                {hasSources && (
                  <div class="text-xs text-[var(--text-muted)] mb-1">From your documents</div>
                )}
                <p class="text-sm text-[var(--text-secondary)]">{props.message.groundingNote}</p>
              </div>
            )}
            {hasSources && (
              <div>
                {hasGrounded && (
                  <div class="text-xs text-[var(--text-muted)] mb-1">From the web</div>
                )}
                <ul class="list-decimal list-inside space-y-2">
                  {props.message.sources?.map((source, index) => (
                    <li key={index}>
                      <a
                        href={source.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        class="text-[var(--text-link)] hover:underline"
                      >
                        {source.title}
                      </a>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {hasGrounded && (
              <div>
                {hasSources && (
                  <div class="text-xs text-[var(--text-muted)] mb-1">From your documents</div>
                )}
                <ul class="space-y-3">
                  {props.message.grounded?.map((g, i) => (
                    <li key={i} class="text-sm">
                      {g.kind === 'document' ? (
                        <>
                          {g.claim && (
                            <div class="text-[var(--text-primary)]">{g.claim}</div>
                          )}
                          {g.quote && (
                            <blockquote class="mt-1 pl-3 border-l-2 border-[var(--border-subtle)] text-[var(--text-secondary)] italic">
                              “{g.quote}”
                            </blockquote>
                          )}
                          <div class="mt-1 text-xs text-[var(--text-muted)]">
                            {g.doc_name || 'document'}
                            {g.span
                              ? ` · chars ${g.span[0]}–${g.span[1]}`
                              : ' · quote located by content'}
                          </div>
                        </>
                      ) : (
                        <div class="text-xs text-[var(--text-muted)]">
                          🖼 {g.doc_name || 'image'} · sha256 {g.doc_sha256.slice(0, 12)}…
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
});

const ChatMessage = component$<ChatMessageProps>((props) => {
  const finalText = useSignal('');
  const displayedLineCount = useSignal(0);
  const thinkingPreviewRef = useSignal<HTMLDivElement>();
  const rootRef = useSignal<HTMLDivElement>();
  const hasAnchoredStart = useSignal(false);
  const isGenerationPhase = useSignal(false);
  const thinkingBoxVisible = useSignal(false);
  const dynamicStatus = useSignal('');
  const statusOpacity = useSignal(1);
  const isCodePanelOpen = useSignal(false);

  // Agent work rail: collapsed-stub expansion, shared by the stub itself
  // and the action bar's Steps button.
  const agentRailOpen = useSignal(false);
  const agentWasLoading = useSignal(false);

  // Settle-on-fold: when an agent turn finishes, its story collapses to the
  // stub - a large height change no matter what. Use that moment for one
  // deliberate move: question, stub, and the answer's first lines to the
  // top, the same resting position every turn. Agent turns only.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const loading = track(() => props.message.isLoading);
    if (
      props.message.agentTurn &&
      agentWasLoading.value &&
      !loading &&
      props.isLast &&
      rootRef.value
    ) {
      requestAnimationFrame(() => {
        const question = rootRef.value?.previousElementSibling as HTMLElement | null;
        question?.scrollIntoView({ behavior: 'auto', block: 'start' });
      });
    }
    agentWasLoading.value = !!loading;
  });

  const statusMessages = [
    'Synthesizing information',
    'Structuring the response',
    'Verifying sources',
    'Composing key insights',
    'Cross-referencing data points',
  ];

  // Inject style override for markdown list items (replaces useEffect with empty deps)
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    const styleId = 'chat-message-list-style-override';
    let styleElement = document.getElementById(styleId);

    if (!styleElement) {
      styleElement = document.createElement('style');
      styleElement.id = styleId;
      styleElement.innerHTML = `.markdown-content li > p:first-child { display: inline !important; margin-bottom: 0 !important; }
/* Thinking views: reasoning often carries markdown headings (plan sections).
   Render them as small bold lines - full-size h1/h2 inside the muted little
   thinking box reads as shouting. Paragraph gaps tightened to match. */
.thinking-markdown h1, .thinking-markdown h2, .thinking-markdown h3,
.thinking-markdown h4, .thinking-markdown h5, .thinking-markdown h6 {
  font-size: 1em !important; font-weight: 600 !important; margin: 0.6em 0 0.2em !important;
}
.thinking-markdown p { margin: 0.35em 0 !important; }
.thinking-markdown ul, .thinking-markdown ol { margin: 0.35em 0 !important; }`;
      document.head.appendChild(styleElement);
    }
  });

  // Update finalText whenever content changes (including during streaming)
  useTask$(({ track }) => {
    const content = track(() => props.message.content);
    if (content) {
      let final = (content || '')
        .replace(/<think(?:ing)?[^>]*>[\s\S]*?<\/think(?:ing)?>/gis, '')
        .replace(/<\/?report>/gi, '')
        .replace(/<\/?response[^>]*>/gi, '')
        .replace(/<\|output\|>/gi, '');

      if (/<think(?:ing)?[^>]*>/i.test(final) && !/<\/think(?:ing)?>/i.test(final)) {
        final = final.replace(/<think(?:ing)?[^>]*>.*/is, '');
      }

      final = final
        .replace(/<\/?think(?:ing)?[^>]*$/i, '')
        .replace(/<\/?report[^>]*$/i, '')
        .replace(/<\/?response[^>]*$/i, '')
        .replace(/<\|output\|[^>]*$/i, '')
        .replace(/<[^>]*$/i, '')
        .trim();
      finalText.value = final;
    }
  });

  // Thinking box visibility: show while loading AND thinking content exists
  // Once thinking has been seen, keep showing until loading completes
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const thinking = track(() => props.message.thinking);
    const isLoading = track(() => props.message.isLoading);

    // LIVE thinking box is gated to report mode — conversation/code stay chatty
    // (no live reasoning) even with models that always emit <think>. Capture, the
    // post-gen "Thoughts" button (hasThoughts), and the transcript are unconditional.
    const meaningful =
      !!thinking &&
      thinking.trim().length >= MIN_THINKING_DISPLAY_CHARS &&
      props.message.turnMode === 'report';
    if (isLoading && meaningful) {
      thinkingBoxVisible.value = true;
    }
    if (!isLoading) {
      thinkingBoxVisible.value = false;
    }
  });

  // Generation phase = the model has finished thinking and the answer has begun.
  // Driven by the REAL content transition (answer text appears), not a timer.
  // A 3s timer used to collapse the thinking box, which fired prematurely when a
  // slow model (e.g. an oversized offline model) produced thinking lines more
  // than a few seconds apart. The content signal is correct for any model on any
  // machine, fast or slow — no timing assumptions.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const thinking = track(() => props.message.thinking);
    const isLoading = track(() => props.message.isLoading);
    const answerStarted = track(() => finalText.value.trim() !== '');
    isGenerationPhase.value = !!isLoading && !!thinking && answerStarted;
  });

  // Dynamic status message cycling
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const inGenPhase = track(() => isGenerationPhase.value);

    if (inGenPhase) {
      let messageIndex = 0;
      dynamicStatus.value = statusMessages[messageIndex];
      statusOpacity.value = 1;

      const interval = setInterval(() => {
        statusOpacity.value = 0;
        setTimeout(() => {
          messageIndex = (messageIndex + 1) % statusMessages.length;
          dynamicStatus.value = statusMessages[messageIndex];
          statusOpacity.value = 1;
        }, 300);
      }, 10000);

      cleanup(() => clearInterval(interval));
    } else {
      dynamicStatus.value = '';
    }
  });

  // Line count / animation frame management
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const ft = track(() => finalText.value);
    const msgIsLoading = track(() => props.message.isLoading);
    const msgError = track(() => props.message.error);
    const msgRole = track(() => props.message.role);

    let effectiveContent = '';
    if (msgRole === 'assistant') {
      effectiveContent = ft;
    }

    const lines = effectiveContent.split('\n');
    const totalLines = lines.length;

    if (msgIsLoading || msgError || !effectiveContent) {
      displayedLineCount.value = totalLines;
      return;
    }

    if (displayedLineCount.value >= totalLines) {
      return;
    }

    // No auto-scroll here: a new reply anchors its question to the top and the
    // reply prints in place below. Chasing the bottom would scroll the question
    // off-screen (it sits within the near-bottom guard once the reply is short).
    displayedLineCount.value = totalLines;
  });

  // Anchor the START of a new assistant response at the top of the viewport
  // when it begins, so long responses (reports especially) are read from the
  // top instead of being scrolled to the end. Fires once per message.
  // Reserve ~a viewport of height on the CURRENT (last) reply so the question
  // above it can scroll all the way to the top with the reply printing below,
  // AND so a finished reply never collapses and jumps. Older messages release
  // it (off-screen, when a newer message arrives) so there are no big gaps.
  // Imperative so it's in the layout before the anchor scroll measures.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const isLast = track(() => props.isLast);
    if (!rootRef.value) return;
    // Agent turns are ONE bubble too (the rail lives inside it), so the
    // same reservation applies - without it the question only reaches the
    // top once enough rail content happens to have streamed, which made
    // follow-up anchoring a coin flip.
    if (isLast && props.message.role === 'assistant') {
      const container = rootRef.value.closest('.overflow-y-auto') as HTMLElement | null;
      rootRef.value.style.minHeight = container ? `${container.clientHeight}px` : '';
    } else {
      rootRef.value.style.minHeight = '';
    }
  });

  // As soon as the pending assistant bubble appears (on Enter — before the model
  // even starts), bring the user's QUESTION (the message above) to the top, so the
  // question + the new bubble/action bar are in view immediately and the reply then
  // prints in place below — no second move. Fires once per reply. (Anchoring on the
  // loading bubble, not on first content, is what makes the view jump on send.)
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const loading = track(() => props.message.isLoading);
    if (
      props.message.role === 'assistant' &&
      loading &&
      !hasAnchoredStart.value &&
      rootRef.value
    ) {
      hasAnchoredStart.value = true;
      // Defer a frame so the user-question sibling is in the DOM + laid out, then
      // jump to put the QUESTION at the top, with the new bubble + action bar
      // right below it. Two hard-won details: the height reservation must be in
      // the layout BEFORE the jump measures (the sibling task can lose that race
      // on smaller/slower screens, landing the question shy of the top), and the
      // jump must be literally 'instant' — 'auto' obeys the container's
      // scroll-smooth CSS and animates, which mid-stream scroll work cuts short.
      requestAnimationFrame(() => {
        const root = rootRef.value;
        if (!root) return;
        const container = root.closest('.overflow-y-auto') as HTMLElement | null;
        if (container && props.isLast && props.message.role === 'assistant') {
          root.style.minHeight = `${container.clientHeight}px`;
        }
        const question = root.previousElementSibling as HTMLElement | null;
        question?.scrollIntoView({ behavior: 'instant', block: 'start' });
      });
    }
  });

  // Give every finished code block a Copy button, and shell blocks a
  // "run in your terminal" button. Injected post-render (the markdown is
  // innerHTML; injecting during streaming would fight the reconciler),
  // native listeners so the first click works, re-injected whenever the
  // content re-renders (the data flag dies with the old DOM).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => props.message.isLoading);
    track(() => props.message.content);
    if (props.message.role !== 'assistant' || props.message.isLoading) return;
    const root = rootRef.value;
    if (!root) return;
    const openTerminal$ = props.onOpenTerminal$;
    requestAnimationFrame(() => {
      const COPY_SVG =
        '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
      const CHECK_SVG =
        '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';
      const TERM_SVG =
        '<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/></svg>';
      root.querySelectorAll('.markdown-content pre').forEach((preEl) => {
        const pre = preEl as HTMLElement;
        if (pre.dataset.codeActions) return;
        const code = pre.querySelector('code');
        if (!code) return;
        pre.dataset.codeActions = '1';
        pre.style.position = 'relative';
        const lang = Array.from(code.classList)
          .find((c) => c.startsWith('language-'))
          ?.slice(9)
          .toLowerCase();
        const isShell = !!lang && SHELL_LANGUAGES.includes(lang);
        const bar = document.createElement('div');
        bar.className = 'code-block-actions';
        const copyBtn = document.createElement('button');
        copyBtn.type = 'button';
        copyBtn.title = 'Copy';
        copyBtn.innerHTML = COPY_SVG;
        copyBtn.addEventListener('click', async () => {
          try {
            const { copyText } = await import('../utils/clipboard');
            await copyText(commandText(code.innerText));
            copyBtn.innerHTML = CHECK_SVG;
            setTimeout(() => (copyBtn.innerHTML = COPY_SVG), 1500);
          } catch (e) {
            console.warn('[ChatMessage] copy failed:', e);
          }
        });
        bar.appendChild(copyBtn);
        if (isShell && openTerminal$) {
          const termBtn = document.createElement('button');
          termBtn.type = 'button';
          termBtn.title = 'Run in your terminal';
          termBtn.innerHTML = TERM_SVG;
          termBtn.addEventListener('click', () => {
            openTerminal$(commandText(code.innerText));
          });
          bar.appendChild(termBtn);
        }
        pre.appendChild(bar);
      });
    });
  });

  // Auto-scroll thinking preview
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => props.message.thinking);
    track(() => props.message.isLoading);

    if (props.message.role === 'assistant' && props.message.isLoading && thinkingPreviewRef.value) {
      const element = thinkingPreviewRef.value;
      element.scrollTo({ top: element.scrollHeight, behavior: 'smooth' });
    }
  });

  // Derived values (replacing useMemo)
  const codeData = (() => {
    if (props.message.role === 'assistant' && finalText.value) {
      return parseCodeFromMarkdown(finalText.value);
    }
    return null;
  })();

  const contentForDisplay = (() => {
    if (props.message.isLoading && props.message.role === 'assistant') {
      return stripIncompleteCodeBlocks(finalText.value);
    }
    return codeData ? codeData.mainText : finalText.value;
  })();

  // Auto-open side panel for code on desktop
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const cd = track(() => codeData);
    const desktop = track(() => props.isDesktop);
    const msgId = track(() => props.message.id);

    if (cd && desktop && msgId) {
      props.setSidePanelContent$({
        messageId: msgId,
        codeString: cd.codeString,
        language: cd.language,
      });
    }
  });

  const displayAiLabel = props.aiLabel || props.message.aiLabel || props.message.model;
  const displayAiImageUrl =
    props.aiImageUrl !== undefined ? props.aiImageUrl : props.message.aiImageUrl;

  const getSenderName = () => {
    if (props.message.role === 'user') return 'You';
    if (displayAiLabel) {
      if (typeof displayAiLabel === 'string' && displayAiLabel === displayAiLabel.toLowerCase()) {
        return displayAiLabel.charAt(0).toUpperCase() + displayAiLabel.slice(1);
      }
      return displayAiLabel;
    }
    return 'Assistant';
  };

  const aiAvatarClasses =
    'w-[60px] h-[60px] lg:w-[80px] lg:h-[80px] rounded-full object-cover absolute top-0 lg:top-[-5px] left-0 lg:left-[-16px] z-10 border-4 border-[var(--bg-main)]';

  // User message - simple render
  if (props.message.role === 'user') {
    const images = props.message.images;
    return (
      <div class="flex justify-end mb-4 items-start gap-2">
        <div class="bg-[var(--bg-user-message)] chat-bubble-border text-[var(--text-primary)] text-sm mt-1 py-1.5 px-4 leading-loose tracking-wide font-light max-w-[calc(100%)] rounded-2xl">
          {images && images.length > 0 && (
            <div class="flex flex-wrap gap-2 justify-end pt-1.5 pb-1">
              {images.map((src, i) => (
                <img
                  key={i}
                  src={src}
                  alt="attached"
                  width={160}
                  height={120}
                  class="max-h-32 w-auto rounded-lg border border-[var(--border-subtle)] object-cover"
                />
              ))}
            </div>
          )}
          {/* Documents attach as chips: an image IS the content, a PDF is
              a source the AI read - its text goes to the model, never here. */}
          {props.message.attachedFiles && props.message.attachedFiles.length > 0 && (
            <div class="flex flex-wrap gap-1.5 justify-end pt-1.5 pb-1">
              {props.message.attachedFiles.map((name, i) => (
                <span
                  key={i}
                  class="inline-flex items-center gap-1 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-card)] px-2 py-0.5 text-xs text-[var(--text-secondary)]"
                  title={name}
                >
                  <LuFileText class="h-3 w-3 flex-shrink-0" />
                  <span class="max-w-[14rem] truncate">{name}</span>
                </span>
              ))}
            </div>
          )}
          {props.message.content}
        </div>
      </div>
    );
  }

  // Assistant message
  const hasError = !!props.message.error;
  const showThinkingBox = thinkingBoxVisible.value;
  const isWritingCode = !!props.message.isLoading && isInsideIncompleteCodeBlock(finalText.value);

  const contentToRender = contentForDisplay;
  const isThisMessageActiveInSidePanel = props.sidePanelContent?.messageId === props.message.id;

  // Render markdown to HTML string
  const codeAnchorOpen = props.isDesktop
    ? isThisMessageActiveInSidePanel && props.isSidePanelVisible
    : isCodePanelOpen.value;
  const renderedHtml =
    contentToRender && !hasError
      ? (() => {
          const html = renderMarkdownWithCitations(
            contentToRender
              .split('\n')
              .slice(0, displayedLineCount.value)
              .join('\n')
          );
          return codeData
            ? placeCodeAnchor(html, codePreviewHtml(codeData, codeAnchorOpen))
            : html.split(CODE_ANCHOR_TOKEN).join('');
        })()
      : '';

  // Render avatar
  const renderAvatarContent = () => {
    // Use custom blob URL if available (user uploaded a thumbnail)
    if (displayAiImageUrl && displayAiImageUrl.startsWith('blob:')) {
      return <img src={displayAiImageUrl} alt={getSenderName()} class={aiAvatarClasses} />;
    }

    // Use the image URL stored on the message
    if (displayAiImageUrl) {
      return <img src={displayAiImageUrl} alt={getSenderName()} class={aiAvatarClasses} />;
    }
    return (
      <div
        class={`${aiAvatarClasses} bg-[var(--bg-button-secondary)] flex items-center justify-center`}
      >
        <LuBot class="text-gray-500 dark:text-zinc-400" style={{ width: '40px', height: '40px' }} />
      </div>
    );
  };

  return (
    <div
      ref={rootRef}
      class="relative mb-4 scroll-mt-4"
      {...(props.message.role === 'assistant' && props.message.content
        ? { 'data-remember-aiid': props.message.model }
        : {})}
    >
      {renderAvatarContent()}
      <div class="ml-6">
        <div class="space-y-1 flex-grow max-w-[calc(100%)]">
          <div class="font-semibold text-[15px] text-[var(--text-primary)] pl-9 md:pl-10 lg:pl-10 font-varela mt-2">
            {getSenderName()}
          </div>

          {/* Auto-detected mode badge — the classifier switched this turn (transparent + overridable via the sparkle). */}
          {props.message.role === 'assistant' &&
            props.message.turnModeAuto &&
            (props.message.turnMode === 'report' || props.message.turnMode === 'code') && (
              <div class="pl-9 md:pl-10 lg:pl-10 -mt-0.5 text-[11px] text-[var(--text-muted)]">
                ✨ {props.message.turnMode === 'report' ? 'Report' : 'Code'} mode · auto
              </div>
            )}

          {props.message.role === 'assistant' && (
            <>
              {showThinkingBox && (
                <div
                  class={`generic-container mb-2 text-sm text-[var(--text-primary)] italic flex flex-col overflow-hidden transition-all duration-500 ease-in-out rounded-2xl ${isGenerationPhase.value ? 'max-h-8 md:max-h-9' : props.message.turnMode === 'report' ? 'max-h-[60vh]' : 'max-h-[11em]'}`}
                >
                  <div class="flex-shrink-0 pr-3 flex items-center gap-1 md:gap-1.5 pl-9 md:pl-[46px] lg:pl-[46px] h-8 md:h-9">
                    <div class="block md:hidden">
                      <ThemeAwareLottie type="thinking" theme={props.theme} size={14} />
                    </div>
                    <div class="hidden md:block mr-1">
                      <ThemeAwareLottie type="thinking" theme={props.theme} size={18} />
                    </div>
                    <span class="animate-pulse-text status-text-gradient text-[10px] leading-tight md:text-xs">
                      Thinking..
                    </span>
                  </div>
                  {/* Rendered as markdown so the live box matches the
                      post-generation Thoughts view (it used to show raw
                      **markdown** source). Partial markdown mid-stream
                      renders fine - it just completes as chunks arrive. */}
                  <div
                    ref={thinkingPreviewRef}
                    class="flex-grow overflow-y-auto px-3 pb-3 scrollbar-none leading-relaxed font-light text-[var(--text-secondary)] text-xs opacity-80 markdown-content thinking-markdown"
                    dangerouslySetInnerHTML={renderMarkdown(props.message.thinking || '')}
                  />
                </div>
              )}
              {!showThinkingBox && (
                <ActionBar
                  message={props.message}
                  // The bar stays put through the whole turn: status while
                  // working ("is working.."), buttons when done. The rail's
                  // pearl carries the SPECIFIC current action below.
                  isLoading={!!props.message.isLoading}
                  railOpen={agentRailOpen}
                  isModelLoading={props.isModelLoading}
                  isWritingCode={isWritingCode}
                  isThinking={!!props.message.thinking && !finalText.value}
                  isGenerationPhase={isGenerationPhase.value}
                  statusText={props.message.statusText}
                  dynamicStatus={dynamicStatus.value}
                  statusOpacity={statusOpacity.value}
                  aiName={getSenderName()}
                  theme={props.theme}
                  onGround$={props.onGround$}
                  isLast={props.isLast}
                  onRouteRetry$={props.onRouteRetry$}
                  canRouteOnline={props.canRouteOnline}
                />
              )}
              {/* Agent work rail: the turn's story (speech, thread, cards),
                  live while working, folded to a stub when done. */}
              {props.message.agentLog && props.message.agentLog.length > 0 && (
                <div class="pl-0 md:pl-10 lg:pl-10 mt-1 mb-1">
                  <AgentWorkingBox
                    log={props.message.agentLog}
                    working={!!props.message.isLoading}
                    tipHere={props.isLast !== false}
                    railOpen={agentRailOpen}
                    retryStatus={props.isLast ? props.agentRetryStatus : undefined}
                    waitingOn={props.isLast ? props.agentWaitingOn : undefined}
                    durationMs={props.message.agentStats?.durationMs}
                    undone={!!props.message.undone}
                    onUndoTurn$={
                      props.isLast && props.onUndoTurn$ && !props.message.undone
                        ? $(() => props.onUndoTurn$!(props.message.id))
                        : undefined
                    }
                    onPermissionRespond$={props.onPermissionRespond$}
                    onPermissionOffscreen$={props.onPermissionOffscreen$}
                  />
                </div>
              )}
              {contentToRender && !hasError && (
                <div
                  // Agent turns speak unboxed - the two registers (words vs
                  // work) carry the hierarchy, not a card background.
                  class={`markdown-content ${props.message.agentTurn ? '' : 'bg-[var(--bg-assistant-message)]'} p-2 pr-2 pb-2 pt-2 pl-0 md:pl-10 lg:pl-10 rounded-lg text-[var(--text-primary)] text-base leading-relaxed ${props.message.thinking ? 'mt-2' : ''}`}
                  dangerouslySetInnerHTML={renderedHtml}
                  // The inline code chip lives in the rendered HTML: handle
                  // its click here by delegation. Desktop toggles the side
                  // panel; narrow screens open the overlay below the message.
                  onClick$={(ev) => {
                    const target = ev.target as HTMLElement | null;
                    if (!target?.closest?.('[data-code-anchor]') || !codeData) return;
                    if (props.isDesktop && props.message.id) {
                      if (isThisMessageActiveInSidePanel && props.isSidePanelVisible) {
                        props.setIsSidePanelVisible$(false);
                      } else {
                        props.setSidePanelContent$({
                          messageId: props.message.id,
                          codeString: codeData.codeString,
                          language: codeData.language,
                        });
                        props.setIsSidePanelVisible$(true);
                      }
                    } else {
                      isCodePanelOpen.value = !isCodePanelOpen.value;
                    }
                  }}
                />
              )}
              {props.message.stopped && (
                <p class="mt-1 pl-0 md:pl-10 lg:pl-10 text-xs text-[var(--text-muted)]">
                  Stopped here - kept in your records as far as it got.
                </p>
              )}
              {codeData && !props.isDesktop && isCodePanelOpen.value && (
                <div class="mt-2 pl-2 md:pl-10 lg:pl-10">
                  <CodePanel
                    codeString={codeData.codeString}
                    language={codeData.language}
                    onClose$={() => {
                      isCodePanelOpen.value = false;
                    }}
                    isOverlay={true}
                    theme={props.theme}
                  />
                </div>
              )}
            </>
          )}
        </div>

        {props.message.error && (
          <div class="mt-2 text-red-500 dark:text-red-400 text-sm flex items-center gap-2 pl-2 md:pl-10 lg:pl-10">
            <span>{props.message.error}</span>

            {props.message.showUpgradeButton &&
              props.onUpgradeClick$ &&
              props.message.originalUserQuery &&
              props.message.id && (
                <button
                  onClick$={() =>
                    props.onUpgradeClick$!(
                      props.message.originalUserQuery!,
                      props.message.id!
                    )
                  }
                  class="px-3 py-1.5 text-xs bg-sky-500 hover:bg-sky-600 text-white rounded-md transition-colors flex-shrink-0 flex items-center gap-1.5"
                >
                  <LuArrowUpCircle style={{ width: '14px', height: '14px' }} />
                  Upgrade Plan
                </button>
              )}

            {!props.message.showUpgradeButton &&
              props.message.role === 'assistant' &&
              props.message.originalUserMessageContent &&
              props.onRetry$ && (
                <button
                  onClick$={() => props.onRetry$!(props.message.originalUserMessageContent)}
                  class="px-2 py-1 text-xs bg-[var(--bg-button-secondary)] hover:bg-[var(--bg-button-secondary-hover)] border border-[var(--border-secondary)] hover:border-[var(--border-secondary-hover)] text-[var(--text-button-secondary)] hover:text-[var(--text-button-secondary-hover)] rounded transition-colors flex-shrink-0"
                >
                  Retry
                </button>
              )}
          </div>
        )}

      </div>
    </div>
  );
});

export { ChatMessage };
export default ChatMessage;
