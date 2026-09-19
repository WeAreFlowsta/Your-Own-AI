import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { lastKnownEntitled } from '../utils/entitlement';

/**
 * The one sentence that follows an honest "this computer can't do that".
 *
 * A DOOR, NOT A PITCH. It says what is true - online models do not depend on
 * this computer's hardware - says plainly that they are an optional paid
 * service, and links to the Online Models page (which explains and gates
 * itself). It never links toward a checkout, never names a price, never
 * appears on a problem that is OURS (an engine crash is not the moment to
 * mention a paid plan), and never on a health question, which stays on the
 * device whatever the mode.
 *
 * Someone who already has a plan is told how to use it instead.
 */
export default component$<{ class?: string }>((props) => {
  const entitled = useSignal<'yes' | 'no' | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    try {
      entitled.value = lastKnownEntitled();
    } catch {
      entitled.value = null;
    }
  });

  return (
    <p class={props.class ?? 'text-sm'}>
      {entitled.value === 'yes' ? (
        <>
          Your plan's{' '}
          <a href="/online-models" class="text-[var(--text-link)] hover:underline">
            online models
          </a>{' '}
          run at full speed on any computer. Set this AI to Auto - Online and
          Offline to use them.
        </>
      ) : (
        <>
          <a href="/online-models" class="text-[var(--text-link)] hover:underline">
            Online models
          </a>{' '}
          run at full speed on any computer, whatever its hardware. They are an
          optional paid service. Everything offline stays free.
        </>
      )}
    </p>
  );
});
