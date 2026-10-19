/**
 * Whose memory: a row of faces at the top of Your Memory and of each AI's
 * memory page. "You" (what every AI knows about you) first, then each AI,
 * with how many things it keeps for itself. Switching faces is moving
 * between memories - the layout teaches that the AIs keep their own memory
 * apart from yours, with no sentence to read (a person, 10-20: the Memory
 * page held nothing that told him the AIs remember on their own).
 */
import { component$, useSignal, useVisibleTask$, type QRL } from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { LuUsers } from "@qwikest/icons/lucide";
import { useAiData } from "../contexts/AiDataContext";

interface Props {
  /** "you", or the AI id whose memory is open. */
  selected: string;
  /** On an AI's memory page: switch in place. Elsewhere the row navigates. */
  onPick$?: QRL<(aiId: string, agentKey: string) => void>;
}

export default component$<Props>((props) => {
  const nav = useNavigate();
  const aiData = useAiData();
  /** Rows each AI keeps for itself, by AI id (from the saved copy: instant). */
  const counts = useSignal<Record<string, number>>({});

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const ais = track(() => aiData.userDefinedAis);
    try {
      const { listMemoryRowsCached, aiScope } = await import("../utils/memoryRows");
      const out: Record<string, number> = {};
      for (const ai of ais) {
        const founding = ai.foundingKey || ai.agentPubKey;
        if (!founding) continue;
        const rows = await listMemoryRowsCached(aiScope(founding)).catch(() => []);
        out[ai.id] = rows.filter((r) => !r.deleted).length;
      }
      counts.value = out;
    } catch {
      /* counts are a nicety */
    }
  });

  const ais = aiData.userDefinedAis.filter((a) => a.status !== "archived");
  const ring = (on: boolean) =>
    on ? "ring-2 ring-[var(--bg-button-primary)]" : "ring-1 ring-[var(--border-subtle)] opacity-70 group-hover:opacity-100";

  return (
    <div class="flex items-start gap-4 overflow-x-auto pb-2 mb-6" data-testid="memory-owners">
      <button
        type="button"
        class="group flex flex-col items-center gap-1 shrink-0 bg-transparent border-none cursor-pointer p-0"
        title="Your Memory - what every AI knows about you"
        data-testid="memory-owner-you"
        data-selected={props.selected === "you" ? "yes" : "no"}
        onClick$={() => {
          if (props.selected !== "you") nav("/your-memory/");
        }}
      >
        <span class={`w-11 h-11 rounded-full flex items-center justify-center bg-gradient-to-br from-purple-500 to-indigo-600 text-white ${ring(props.selected === "you")}`}>
          <LuUsers class="w-5 h-5" />
        </span>
        <span class={`text-[11px] ${props.selected === "you" ? "text-[var(--text-primary)] font-semibold" : "text-[var(--text-muted)]"}`}>You</span>
      </button>
      <span class="self-center h-8 w-px bg-[var(--border-subtle)] shrink-0" aria-hidden="true" />
      {ais.map((ai) => {
        const on = props.selected === ai.id;
        const url = aiData.thumbnailObjectUrls[ai.id];
        const n = counts.value[ai.id] ?? 0;
        return (
          <button
            key={ai.id}
            type="button"
            class="group flex flex-col items-center gap-1 shrink-0 bg-transparent border-none cursor-pointer p-0"
            title={`${ai.name}'s own memory`}
            data-testid="memory-owner-ai"
            data-ai-id={ai.id}
            data-selected={on ? "yes" : "no"}
            onClick$={async () => {
              if (on) return;
              if (props.onPick$) {
                await props.onPick$(ai.id, ai.agentPubKey || "");
                return;
              }
              sessionStorage.setItem("memory-ai-id", ai.id);
              sessionStorage.setItem("memory-agent-key", ai.agentPubKey || "");
              sessionStorage.setItem("memory-tab", "knows");
              nav("/memory/");
            }}
          >
            <span class="relative">
              {url ? (
                <img src={url} alt={ai.name} width={44} height={44} class={`w-11 h-11 rounded-full object-cover ${ring(on)}`} />
              ) : (
                <span class={`w-11 h-11 rounded-full flex items-center justify-center bg-gradient-to-br from-purple-500 to-indigo-600 text-white font-bold ${ring(on)}`}>
                  {ai.name.charAt(0).toUpperCase()}
                </span>
              )}
              {n > 0 && (
                <span class="absolute -bottom-1 -right-1 min-w-[18px] h-[18px] px-1 rounded-full bg-[var(--bg-card)] border border-[var(--border-subtle)] text-[10px] leading-[16px] text-center text-[var(--text-secondary)] tabular-nums" data-testid="memory-owner-count">
                  {n}
                </span>
              )}
            </span>
            <span class={`text-[11px] max-w-[64px] truncate ${on ? "text-[var(--text-primary)] font-semibold" : "text-[var(--text-muted)]"}`}>{ai.name}</span>
          </button>
        );
      })}
    </div>
  );
});
