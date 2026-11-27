/**
 * "Show more memory controls" (Settings, Memory): off unless the person
 * turns it on. Memory works the same either way; on, the screens also
 * show the controls for looking deeper and repairing by hand (move an
 * item, choose who can use it, see where it came from, leave a passage
 * out, read the exact text given, how tidying is going). A choice about
 * what one screen shows, so it is kept on this device.
 */
import { useSignal, useVisibleTask$, type Signal } from "@builder.io/qwik";

const KEY = "yoai-memory-more";
export const MEMORY_MORE_EVENT = "yoai-memory-more";

export function moreMemoryControls(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function setMoreMemoryControls(on: boolean): void {
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    /* the screens stay as they are */
  }
  try {
    window.dispatchEvent(new CustomEvent(MEMORY_MORE_EVENT));
  } catch {
    /* no window */
  }
}

/** The choice as a signal that follows the switch while the page is open. */
export function useMoreMemoryControls(): Signal<boolean> {
  const more = useSignal(false);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const read = () => (more.value = moreMemoryControls());
    read();
    window.addEventListener(MEMORY_MORE_EVENT, read);
    cleanup(() => window.removeEventListener(MEMORY_MORE_EVENT, read));
  });
  return more;
}
