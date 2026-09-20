import { useSignal, useVisibleTask$, type QRL, type Signal } from '@builder.io/qwik';
import { claimFileDrops, releaseFileDrops } from '../utils/fileDrops';

/**
 * Window drag-and-drop for a section: Tauri hands over real file paths, so
 * the section can read the files the same way the picker does. Claims the
 * drops while mounted so nothing else on the page reacts to them. Returns a
 * signal that is true while files hover over the window.
 */
export function useFileDrop(owner: string, onPaths$: QRL<(paths: string[]) => void>): Signal<boolean> {
  const hovering = useSignal(false);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    claimFileDrops(owner);
    let unlisten: (() => void) | null = null;
    // Registration is async: a section removed before it lands must still
    // drop its listener, or a dead copy keeps catching drops (and refreshes
    // a list nobody sees).
    let gone = false;
    import('@tauri-apps/api/webviewWindow').then(({ getCurrentWebviewWindow }) => {
      getCurrentWebviewWindow()
        .onDragDropEvent((event) => {
          const t = event.payload.type;
          if (t === 'enter' || t === 'over') hovering.value = true;
          else if (t === 'leave') hovering.value = false;
          else if (t === 'drop') {
            if (gone) return;
            hovering.value = false;
            if (event.payload.paths.length > 0) void onPaths$(event.payload.paths);
          }
        })
        .then((fn) => {
          if (gone) fn();
          else unlisten = fn;
        });
    });
    cleanup(() => {
      gone = true;
      releaseFileDrops(owner);
      if (unlisten) unlisten();
    });
  });
  return hovering;
}
