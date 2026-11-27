import { component$, $, useSignal } from '@builder.io/qwik';

/**
 * Where a reply says memory did not look because the person turned it off
 * on this computer: the switch, in place. Off is for a computer on which
 * looking through memory makes each reply start late; conversations are
 * still kept and read, and a faster device uses them.
 */
export const MemoryHere = component$(() => {
  const state = useSignal<'' | 'on' | 'failed'>('');

  const turnOn = $(async () => {
    try {
      const { setMemoryInReplies } = await import('../../utils/memoryKeep');
      await setMemoryInReplies(true);
      state.value = 'on';
    } catch {
      state.value = 'failed';
    }
  });

  return (
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="memory-here" data-state={state.value}>
      {state.value === 'on' ? (
        <span>Memory is on for this computer. Ask again and memory will look.</span>
      ) : (
        <>
          {state.value === 'failed' && <span>Could not turn it on just now.</span>}
          <button type="button" class="underline" data-testid="memory-here-on" onClick$={turnOn}>
            Turn it on
          </button>
        </>
      )}
    </div>
  );
});
