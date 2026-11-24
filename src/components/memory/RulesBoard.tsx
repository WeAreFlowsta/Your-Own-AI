/**
 * Rules: what the person's AIs always follow. A rule is written here in
 * the person's own words, listed with the rest, and given to every AI
 * with every answer. Below: the offers the person declined.
 */
import { component$, useSignal, $ } from '@builder.io/qwik';
import LiquidMetalButton from '../LiquidMetalButton';
import MemoryRowsPanel from '../MemoryRowsPanel';
import { DeclinedOffers } from './DeclinedOffers';
import { saveMemoryRow, newRow, personScope, projectScope, listMemoryRows } from '../../utils/memoryRows';
import type { MemoryProject } from '../../utils/memoryProject';

/** `project`: the project Memory is opened on. Its rules stand first, with
 *  their own field; they are given in that project's sessions only. */
export const RulesBoard = component$<{ project?: MemoryProject | null }>((props) => {
  const projectDraft = useSignal('');
  const projectRefresh = useSignal(0);
  const agentsMd = useSignal('');

  const addForProject = $(async () => {
    const text = projectDraft.value.trim();
    const id = props.project?.id;
    if (!text || !id) return;
    try {
      await saveMemoryRow(newRow(projectScope(id), 'rule', text, { pinned: true }));
      projectDraft.value = '';
      projectRefresh.value++;
    } catch (e) {
      agentsMd.value = `The rule could not be saved just now. ${String(e)}`;
    }
  });

  // The project's pinned rules, added to the folder's AGENTS.md: the file
  // coding tools read. Never written unasked.
  const saveAsAgentsMd = $(async () => {
    const project = props.project;
    if (!project) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const rows = await listMemoryRows(projectScope(project.id));
      const lines = rows.filter((r) => r.kind === 'rule' && !r.deleted && r.pinned).map((r) => r.text);
      if (!lines.length) return void (agentsMd.value = 'No pinned rules to save yet.');
      const r = await invoke<{ added: number }>('save_rules_as_agents_md', { folder: project.folder, lines });
      agentsMd.value = r.added ? `Added ${r.added} line${r.added === 1 ? '' : 's'} to AGENTS.md.` : 'AGENTS.md already has these rules.';
    } catch (e) {
      agentsMd.value = `AGENTS.md could not be written: ${String(e)}`;
    }
  });

  const draft = useSignal('');
  const refresh = useSignal(0);
  const failed = useSignal('');
  const saving = useSignal(false);

  const add = $(async () => {
    const text = draft.value.trim();
    if (!text || saving.value) return;
    saving.value = true;
    try {
      await saveMemoryRow(newRow(personScope, 'rule', text, { pinned: true }));
      draft.value = '';
      failed.value = '';
      refresh.value++;
    } catch (e) {
      failed.value = `The rule could not be saved just now. ${String(e)}`;
    }
    saving.value = false;
  });

  return (
    <div class="flex flex-col gap-6" data-testid="rules-board">
      {props.project && (
        <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="project-rules">
          <h3 class="text-lg font-semibold text-[var(--text-primary)]">Rules for {props.project.name}</h3>
          <p class="mt-1 text-sm text-[var(--text-secondary)] mb-3">Your AIs follow these when they work in this project, and nowhere else.</p>
          <div class="flex flex-wrap items-center gap-2 mb-5">
            <input
              type="text"
              class="flex-1 min-w-[220px] px-3 py-2 rounded-lg bg-[var(--bg-input)] border border-[var(--border-input)] text-[var(--text-primary)] text-sm"
              placeholder="For example: Deploy with make ship"
              data-testid="project-rule-add"
              value={projectDraft.value}
              onInput$={(_, el) => (projectDraft.value = el.value)}
              onKeyDown$={(e) => e.key === 'Enter' && addForProject()}
            />
            <LiquidMetalButton class="px-4 py-2 text-sm" testId="project-rule-add-save" onClick$={addForProject}>
              Add rule
            </LiquidMetalButton>
          </div>
          <MemoryRowsPanel scope={projectScope(props.project.id)} refresh={projectRefresh.value} rulesHeading="Rules for this project" />
          <div class="mt-2 flex flex-wrap items-center gap-3 text-xs">
            <button type="button" data-testid="save-as-agents-md" class="text-[var(--text-link)] hover:underline bg-transparent border-none cursor-pointer" onClick$={saveAsAgentsMd}>
              Save rules as AGENTS.md
            </button>
            {agentsMd.value && (
              <span class="text-[var(--text-muted)]" data-testid="agents-md-result">
                {agentsMd.value}
              </span>
            )}
          </div>
        </section>
      )}
      <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
        <p class="text-sm text-[var(--text-secondary)] mb-3">A rule is something your AIs always follow. Write it in your own words.</p>
        <div class="flex flex-wrap items-center gap-2 mb-5">
          <input
            type="text"
            class="flex-1 min-w-[220px] px-3 py-2 rounded-lg bg-[var(--bg-input)] border border-[var(--border-input)] text-[var(--text-primary)] text-sm"
            placeholder="For example: Always ask before sending anything"
            data-testid="rule-add"
            value={draft.value}
            onInput$={(_, el) => (draft.value = el.value)}
            onKeyDown$={(e) => e.key === 'Enter' && add()}
          />
          <LiquidMetalButton class="px-4 py-2 text-sm" testId="rule-add-save" onClick$={add} disabled={saving.value}>
            Add rule
          </LiquidMetalButton>
        </div>
        {failed.value && <p class="text-sm text-[var(--text-secondary)] mb-3">{failed.value}</p>}
        <MemoryRowsPanel scope={personScope} refresh={refresh.value} />
      </section>
      <DeclinedOffers />
    </div>
  );
});
