import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { getHistoryArrival } from '../../utils/holochainTranscripts';

/**
 * The line's words while conversations written on another of the person's
 * devices are still arriving here, or nothing when none are. It says only
 * what this device can count: the conversations it knows of and does not
 * hold yet. Changes to memory made there are not counted, because they are
 * not known here until they arrive.
 */
export function catchingUpWords(notHere: number, device: string | null): string {
  if (notHere <= 0) return '';
  const from = device && device.trim() ? device.trim() : 'your other devices';
  const many = notHere === 1 ? '1 conversation is' : `${notHere.toLocaleString()} conversations are`;
  return `Catching up with ${from}. ${many} still arriving, and changes to memory made there show here as they arrive. Nothing you did here is lost.`;
}

/**
 * Under the tidying line: memory on this device is still catching up with
 * another of the person's devices. Shown only while something is arriving,
 * and it goes by itself.
 */
export const CatchingUpLine = component$(() => {
  const words = useSignal('');

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const load = async () => {
      const arrival = await getHistoryArrival();
      words.value = catchingUpWords(arrival.not_here, arrival.device);
    };
    await load();
    const t = setInterval(() => void load(), 30_000);
    cleanup(() => clearInterval(t));
  });

  if (!words.value) return null;
  return (
    <div class="mb-4 flex items-start gap-3 rounded-xl border border-[var(--border-subtle)] px-4 py-3 text-sm text-[var(--text-secondary)]" data-testid="memory-catching-up" role="status">
      <span class="mt-1.5 h-2 w-2 shrink-0 animate-pulse rounded-full bg-[var(--bg-button-primary)]" aria-hidden="true" />
      <span>{words.value}</span>
    </div>
  );
});
