import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';

type Group = { requests: number; cost_usd: number };
type Breakdown = {
  recent_total: number;
  recent_device: number;
  recent_server: number;
  recent_online: number;
  everyday: Group;
  strong: Group;
  search: Group;
  estimate_all_strong_usd: number | null;
  strongest_name: string | null;
};

const usd = (n: number) => (n < 0.01 && n > 0 ? '<$0.01' : `$${n.toFixed(2)}`);
const qs = (n: number) => `${n} ${n === 1 ? 'question' : 'questions'}`;

/**
 * Settings > Routing: where answers came from. The last decisions on this
 * computer (the routing log), and this plan period's online use by kind of
 * model with its real cost (the proxy's per-model totals). The comparison
 * with the strongest model is an estimate at list prices and says so.
 */
export const RoutingBreakdown = component$(() => {
  const b = useSignal<Breakdown | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    b.value = await invoke<Breakdown>('routing_breakdown').catch(() => null);
  });

  const v = b.value;
  if (!v || v.recent_total === 0) return null;
  const periodGroups = [
    { label: 'everyday model', g: v.everyday },
    { label: 'stronger models', g: v.strong },
    { label: 'web search', g: v.search },
  ].filter((x) => x.g.requests > 0);
  const paid = v.everyday.cost_usd + v.strong.cost_usd;
  const est = v.estimate_all_strong_usd;

  return (
    <details class="mb-5" data-testid="routing-breakdown">
      <summary class="cursor-pointer text-sm text-[var(--text-link)] hover:underline">Where your answers came from</summary>
      <div class="mt-2 space-y-1.5 text-sm text-[var(--text-secondary)]">
        <p>
          Your last {qs(v.recent_total)}: {v.recent_device} on this computer
          {v.recent_server > 0 ? `, ${v.recent_server} on your server` : ''}, {v.recent_online} online.
        </p>
        {periodGroups.length > 0 && (
          <p>
            Online this plan period:{' '}
            {periodGroups.map((x, i) => (
              <span key={x.label}>
                {i > 0 ? ' · ' : ''}
                {x.label} {usd(x.g.cost_usd)} ({qs(x.g.requests)})
              </span>
            ))}
          </p>
        )}
        {est != null && v.strongest_name && v.everyday.requests > 0 && est > paid && (
          <p class="text-xs text-[var(--text-muted)]">
            An estimate at list prices: the same everyday and harder questions all on {v.strongest_name} would
            have cost about {usd(est)}. They cost you {usd(paid)}. Caching, thinking and answer length change real costs.
          </p>
        )}
      </div>
    </details>
  );
});
