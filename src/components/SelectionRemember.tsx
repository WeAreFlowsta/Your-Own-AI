import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import type { RememberHandle } from '../utils/rememberText';

/**
 * Select-to-remember: highlight text inside any assistant reply and a small
 * floating "Remember" chip appears; click it to save the selection (where it
 * goes - that AI's memory or the shared notes - is the Settings → Memory
 * destination). An already-remembered selection shows "Remembered": the chip
 * is a toggle, click again to forget the save. Mounted ONCE at the chat page
 * (one document listener, not one per message), so it adds no per-message
 * chrome and nothing renders until the user deliberately selects text.
 *
 * The chip is pure DOM math (native selection rect) - it never re-renders the
 * message list, so it can't slow streaming or scrolling.
 */
export default component$(() => {
  const visible = useSignal(false);
  const x = useSignal(0);
  const y = useSignal(0);
  const aiId = useSignal('');
  const text = useSignal('');
  const state = useSignal(''); // '' | 'saving' | 'saved' | 'error'
  const handle = useSignal<RememberHandle | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const hide = () => {
      visible.value = false;
      state.value = '';
      handle.value = null;
    };

    // On mouseup (selection settled), decide whether to show the chip.
    const onMouseUp = () => {
      // Defer a tick so the selection is finalized.
      setTimeout(() => {
        const sel = window.getSelection();
        const selected = sel?.toString().trim() ?? '';
        // Meaningful selection only - a stray click or a couple of chars
        // shouldn't pop the chip.
        if (!sel || sel.rangeCount === 0 || selected.length < 8) {
          hide();
          return;
        }
        const anchor = sel.anchorNode;
        const el =
          anchor && (anchor.nodeType === 1 ? (anchor as Element) : anchor.parentElement);
        const host = el?.closest('[data-remember-aiid]') as HTMLElement | null;
        if (!host) {
          hide();
          return;
        }
        const rect = sel.getRangeAt(0).getBoundingClientRect();
        if (rect.width === 0 && rect.height === 0) {
          hide();
          return;
        }
        aiId.value = host.getAttribute('data-remember-aiid') || '';
        text.value = selected;
        // Position centered above the selection, clamped to the viewport.
        x.value = Math.min(Math.max(rect.left + rect.width / 2, 70), window.innerWidth - 70);
        y.value = Math.max(rect.top - 8, 40);
        state.value = '';
        handle.value = null;
        visible.value = true;
        // Already remembered? Flip the chip to its "Remembered" (forget) state.
        import('../utils/rememberText')
          .then(({ findRememberedCached }) =>
            findRememberedCached(aiId.value, selected, 'selection'),
          )
          .then((h) => {
            if (h && visible.value && text.value === selected && state.value === '') {
              handle.value = h;
              state.value = 'saved';
            }
          })
          .catch(() => { /* display-only check */ });
      }, 0);
    };

    // Hide when the selection collapses (click elsewhere) or on scroll.
    const onSelChange = () => {
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed || (sel.toString().trim().length < 8)) hide();
    };
    const onScroll = () => { if (visible.value) hide(); };

    document.addEventListener('mouseup', onMouseUp);
    document.addEventListener('selectionchange', onSelChange);
    window.addEventListener('scroll', onScroll, true);
    cleanup(() => {
      document.removeEventListener('mouseup', onMouseUp);
      document.removeEventListener('selectionchange', onSelChange);
      window.removeEventListener('scroll', onScroll, true);
    });
  });

  const remember = $(async () => {
    if (state.value === 'saving') return;
    if (state.value === 'saved' && handle.value) {
      // Toggle off: forget the save, keep the chip up showing "Remember".
      state.value = 'saving';
      try {
        const { forgetRemembered } = await import('../utils/rememberText');
        await forgetRemembered(aiId.value, handle.value);
        handle.value = null;
        state.value = '';
      } catch {
        state.value = 'saved';
      }
      return;
    }
    state.value = 'saving';
    try {
      const { rememberText } = await import('../utils/rememberText');
      const h = await rememberText(aiId.value, text.value, 'selection');
      handle.value = h;
      state.value = h ? 'saved' : 'error';
    } catch {
      state.value = 'error';
    }
    if (state.value === 'saved') {
      setTimeout(() => {
        // Unless the user toggled it off again in the meantime.
        if (state.value === 'saved') visible.value = false;
      }, 900);
    }
  });

  if (!visible.value) return null;

  return (
    <button
      type="button"
      // Keep the mousedown from clearing the selection before the click fires.
      preventdefault:mousedown
      onClick$={remember}
      style={{
        position: 'fixed',
        left: `${x.value}px`,
        top: `${y.value}px`,
        transform: 'translate(-50%, -100%)',
        zIndex: 60,
      }}
      class="flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium bg-[var(--bg-dropdown)] text-[var(--text-primary)] border border-[var(--border-subtle)] shadow-xl hover:bg-[var(--bg-card)] transition-colors"
    >
      <svg viewBox="0 0 16 16" width="13" height="13" fill="currentColor" aria-hidden="true">
        <path d="M2 2a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v13.5a.5.5 0 0 1-.777.416L8 13.101l-5.223 2.815A.5.5 0 0 1 2 15.5V2zm2-1a1 1 0 0 0-1 1v12.566l4.723-2.482a.5.5 0 0 1 .554 0L13 14.566V2a1 1 0 0 0-1-1H4z" />
      </svg>
      {state.value === 'saving'
        ? 'Saving…'
        : state.value === 'saved'
          ? 'Remembered'
          : state.value === 'error'
            ? 'Try again'
            : 'Remember'}
    </button>
  );
});
