import { component$, useSignal, useVisibleTask$, $, type QRL, type Signal } from '@builder.io/qwik';

import type { ChatAction, AttachedFile } from '../types';
import LiquidMetalButton from './LiquidMetalButton';
import { ActionMenu } from './ActionMenu';

interface ContentEditorProps {
  input: Signal<string>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  currentPlaceholder: string;
  selectedAction: ChatAction;
  setSelectedAction$: QRL<(action: ChatAction) => void>;
  stopChat$?: QRL<() => void>;
  attachedFiles: Signal<AttachedFile[]>;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  /** Id of the currently selected AI — when it changes, the editor takes focus. */
  selectedAiId?: string;
}

export const ContentEditor = component$<ContentEditorProps>((props) => {
  const isMultiLine = useSignal(false);
  const singleLineHeight = useSignal<number | null>(null);
  const contentEditableRef = useSignal<HTMLElement>();
  const hasContent = useSignal(false);
  const lastSyncedInput = useSignal(props.input.value);
  // Tracks the last AI we focused for, so a real switch focuses but the initial
  // mount doesn't double-trigger with the mount-time focus.
  const lastFocusedAiId = useSignal<string | undefined>(undefined);

  // Mount-once: ResizeObserver + initial focus
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const observer = new ResizeObserver(entries => {
      for (const entry of entries) {
        const { height } = entry.contentRect;
        if (singleLineHeight.value === null && height > 0) {
          singleLineHeight.value = height;
        }
        if (singleLineHeight.value !== null) {
          isMultiLine.value = height > singleLineHeight.value + 5;
        }
      }
    });

    const el = contentEditableRef.value;
    let removePaste: (() => void) | null = null;
    if (el) {
      observer.observe(el);
      el.focus();

      // EAGER native paste handler — attached on mount so it's ready for the
      // FIRST Ctrl+V. Qwik's onPaste$ is lazy-loaded, so on a fresh field the
      // first paste fires before the handler exists (preventdefault blocks the
      // rich paste but nothing inserts). This strips to plain text — rich
      // clipboard HTML (e.g. VS Code code blocks) would otherwise drop styled
      // <span>s — and inserts at the caret via Range (execCommand('insertText')
      // is unreliable in WebKitGTK), then syncs the value by hand.
      const onPasteNative = (e: ClipboardEvent) => {
        e.preventDefault();
        const text = e.clipboardData?.getData('text/plain') ?? '';
        if (!text) return;
        const sel = window.getSelection();
        if (sel && sel.rangeCount > 0 && el.contains(sel.anchorNode)) {
          const range = sel.getRangeAt(0);
          range.deleteContents();
          const frag = document.createDocumentFragment();
          text.split('\n').forEach((line, i) => {
            if (i > 0) frag.appendChild(document.createElement('br'));
            if (line) frag.appendChild(document.createTextNode(line));
          });
          const last = frag.lastChild;
          range.insertNode(frag);
          if (last) {
            range.setStartAfter(last);
            range.collapse(true);
            sel.removeAllRanges();
            sel.addRange(range);
          }
        } else {
          el.appendChild(document.createTextNode(text));
        }
        const plainText = (el.innerText ?? '').replace(/\u200B/g, '');
        lastSyncedInput.value = plainText;
        props.input.value = plainText;
        hasContent.value = !!plainText.trim();
      };
      el.addEventListener('paste', onPasteNative);
      removePaste = () => el.removeEventListener('paste', onPasteNative);
    }

    // Listen for Tauri drag-drop events to get full file paths
    let unlisten: (() => void) | null = null;
    import('@tauri-apps/api/webviewWindow').then(({ getCurrentWebviewWindow }) => {
      getCurrentWebviewWindow().onDragDropEvent((event) => {
        if (event.payload.type === 'drop' && event.payload.paths.length > 0) {
          props.onAttachFiles$(event.payload.paths);
        }
      }).then((fn) => { unlisten = fn; });
    });

    cleanup(() => {
      if (el) observer.unobserve(el);
      if (removePaste) removePaste();
      if (unlisten) unlisten();
    });
  });

  // Reactive: sync contenteditable when input is cleared externally
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const currentInput = track(() => props.input.value);

    const el = contentEditableRef.value;
    if (!el) return;

    if (currentInput === '' && lastSyncedInput.value !== '') {
      el.innerHTML = '';
      hasContent.value = false;
      setTimeout(() => { el.focus(); }, 0);
    }
    lastSyncedInput.value = currentInput;
  });

  // Reactive: when the user switches AI in the selector, drop the caret into the
  // input so they can type straight away (no extra click). Skips the initial mount
  // (the field already focuses on mount) and never steals focus mid-reply.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const aiId = track(() => props.selectedAiId);
    const isInitial = lastFocusedAiId.value === undefined;
    const changed = !isInitial && lastFocusedAiId.value !== aiId;
    lastFocusedAiId.value = aiId;
    if (!changed || props.isLoading) return;

    const el = contentEditableRef.value;
    if (!el) return;
    el.focus();
    // Place the caret at the end of whatever's there (handles empty, existing text,
    // or an action chip's zero-width space — keeps it to the RIGHT of the chip).
    const sel = window.getSelection();
    if (sel) {
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      sel.removeAllRanges();
      sel.addRange(range);
    }
  });

  // Reactive: when an action is selected, seed the empty editor with a zero-width
  // space. An empty contenteditable drops its caret at the far LEFT (behind the
  // floated action chip) until you type — the ZWSP gives the caret a real text
  // anchor so it renders to the RIGHT of the chip immediately (verified via headless
  // layout measurement: caret x lands past the chip's right edge). The ZWSP is
  // stripped from the extracted text (onContentChange) so it never reaches the prompt.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const action = track(() => props.selectedAction);
    const el = contentEditableRef.value;
    if (!el) return;

    const userText = (el.textContent ?? '').replace(/\u200B/g, '');

    if (action) {
      // Only seed when nothing has been typed yet (don't clobber existing text).
      if (userText.trim() === '') {
        el.textContent = '\u200B';
        el.focus();
        const sel = window.getSelection();
        if (sel && el.firstChild) {
          const range = document.createRange();
          range.setStart(el.firstChild, 1); // caret immediately after the ZWSP
          range.collapse(true);
          sel.removeAllRanges();
          sel.addRange(range);
        }
      }
    } else if (el.textContent === '\u200B') {
      // Action removed with nothing typed — clear the lone seed.
      el.textContent = '';
    }
  });

  const onContentChange = $((e: InputEvent) => {
    const target = e.target as HTMLElement;
    // innerText gives the rendered text including nested nodes (e.g. a <span> from
    // a rich paste) and block\u2192newline, and excludes the sibling tag chip. The old
    // reader only walked top-level text/<br>/<div>/<p>, so span text read as EMPTY
    // \u2014 value blank, placeholder overlapping. Verified reliable in WebKitGTK.
    const plainText = (target.innerText ?? '').replace(/\u200B/g, '');
    if (plainText !== props.input.value) {
      lastSyncedInput.value = plainText;
      props.input.value = plainText;
    }
    hasContent.value = !!plainText.trim();
  });

  const onDrop = $((e: DragEvent) => {
    e.preventDefault();
  });


  const handleContainerClick = $((e: MouseEvent) => {
    const target = e.target as HTMLElement;
    if (contentEditableRef.value && !contentEditableRef.value.contains(target) && !target.closest('.action-tag')) {
      contentEditableRef.value.focus();
    }
  });

  const handleEditorKeyDown = $((e: KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      props.handleSubmit$();
      return;
    }
    if (e.key === 'Backspace' && props.selectedAction) {
      // No user text yet (only the ZWSP seed) → Backspace removes the action.
      const el = contentEditableRef.value;
      if (el && (el.textContent ?? '').replace(/\u200B/g, '').trim() === '') {
        e.preventDefault();
        props.setSelectedAction$(null);
      }
    }
  });

  return (
    <div
      class={`relative flex ${isMultiLine.value ? 'items-start' : 'items-center'} rounded-3xl gradient-border-target transition-[padding,align-items] duration-200 p-1 bg-[var(--bg-input)]`}
      onClick$={handleContainerClick}
      preventdefault:dragover
    >
      {/* Action menu */}
      <div class={`flex shrink-0 ${isMultiLine.value ? 'items-start mt-1' : 'items-center'}`}>
        <ActionMenu
          setSelectedAction$={props.setSelectedAction$}
          onAttachFiles$={props.onAttachFiles$}
          isMultiLine={isMultiLine.value}
        />
      </div>

      <div class="flex-grow flex-shrink min-w-0 relative">
        <div class={`w-full h-full chat-input-grid-container ${isMultiLine.value ? 'multi-line' : ''}`}>

          {/* Scrolling Layer */}
          <div class="w-full h-full max-h-[120px] overflow-y-auto custom-scrollbar">
            {/* Padded Content Layer */}
            <div class="pr-20 sm:pr-28">
              {props.selectedAction && (
                <span
                  contentEditable="false"
                  class="action-tag float-left mr-1.5"
                >
                  {props.selectedAction}
                  <button
                    type="button"
                    onClick$={() => props.setSelectedAction$(null)}
                    class="ml-1.5 cursor-pointer"
                    aria-label="Remove action"
                  >
                    &times;
                  </button>
                </span>
              )}
              <div
                ref={contentEditableRef}
                contentEditable={props.isLoading ? 'false' : 'true'}
                onInput$={onContentChange}
                onDrop$={onDrop}
                onKeyDown$={handleEditorKeyDown}
                class="w-full h-full focus:outline-none text-xs sm:text-base pl-0 pr-2 py-1 text-left break-words leading-[1.4] sm:leading-[1.4] text-[var(--text-main)] caret-[var(--text-main)]"
              />
            </div>
          </div>

          {!hasContent.value && !props.selectedAction && (
            <div
              class="absolute top-0 left-2 py-1 text-xs sm:text-base pointer-events-none text-[var(--text-secondary)]"
            >
              {props.currentPlaceholder}
            </div>
          )}

          <div class={`transition-all duration-200 justify-self-end ${isMultiLine.value ? 'self-end mr-1.5 mb-1.5' : 'self-center mr-1.5'}`}>
            {props.isLoading && props.stopChat$ ? (
              <LiquidMetalButton
                variant="danger"
                onClick$={props.stopChat$}
                class="flex items-center gap-1 px-3 py-1 transition-colors cursor-pointer"
              >
                <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M4 4h16v16H4z" />
                </svg>
                <span>Stop</span>
              </LiquidMetalButton>
            ) : (
              <LiquidMetalButton
                type="submit"
                class="flex items-center gap-1.5 px-3 py-1.5 text-sm transition-all duration-150 ease-in-out disabled:opacity-50 disabled:cursor-not-allowed"
                disabled={props.isLoading || !props.input.value.trim()}
              >
                <svg class="w-[18px] h-[18px]" viewBox="0 0 16 16" fill="currentColor">
                  <path d="M2.678 11.894a1 1 0 0 1 .287.801 10.97 10.97 0 0 1-.398 2c1.395-.323 2.247-.697 2.634-.893a1 1 0 0 1 .71-.074A8.06 8.06 0 0 0 8 14c3.996 0 7-2.807 7-6s-3.004-6-7-6-7 2.808-7 6c0 1.468.617 2.83 1.678 3.894zm-.493 3.905a21.682 21.682 0 0 1-.713.129c-.2.032-.352-.176-.273-.362a9.68 9.68 0 0 0 .244-.637l.003-.01c.248-.72.45-1.548.524-2.319C.743 11.37 0 9.76 0 8c0-3.866 3.582-7 8-7s8 3.134 8 7-3.582 7-8 7a9.06 9.06 0 0 1-2.347-.306c-.52.263-1.639.742-3.468 1.105z" />
                </svg>
                <span class="hidden sm:inline">Ask</span>
              </LiquidMetalButton>
            )}
          </div>

        </div>
      </div>

    </div>
  );
});
