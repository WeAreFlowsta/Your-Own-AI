/**
 * The Base Model choice rules, shared by every surface that offers the
 * choice: the AI edit modal, the model chip on Your AIs cards, and the
 * chip beside the chat's AI selector. The RULES live here so the
 * surfaces cannot drift - each renders its own UI from these.
 */

export interface AutoOption {
  id: string;
  label: string;
  hint: string;
}

/**
 * The Auto (smart routing) choices. Conditional options follow one rule
 * everywhere: offered when available, and an AI already SET to one keeps
 * it visible so editing never strips the setting.
 */
export function autoOptions(args: {
  hasExternal: boolean;
  hasOnlineModels: boolean;
  onlineEntitled: boolean;
  currentModel: string;
}): AutoOption[] {
  const opts: AutoOption[] = [
    { id: 'auto:offline', label: 'Auto — Offline Only', hint: 'best of your offline models' },
  ];
  if (args.hasExternal || args.currentModel === 'auto:my-hardware') {
    opts.push({
      id: 'auto:my-hardware',
      label: 'Auto — My Hardware',
      hint: 'your device + your connected server',
    });
  }
  if (
    args.hasOnlineModels &&
    (args.onlineEntitled || args.currentModel === 'auto:online-offline')
  ) {
    opts.push({
      id: 'auto:online-offline',
      label: 'Auto — Online and Offline',
      hint: 'offline, + online for up-to-date questions',
    });
  }
  return opts;
}

/**
 * Online models actually offered: with a plan; the one already selected
 * stays visible either way; paused ones hide unless selected.
 */
export function offeredOnlineModels<T extends { id: string }>(
  models: T[],
  onlineEntitled: boolean,
  currentModel: string,
  isPaused: (id: string) => boolean,
): T[] {
  return models
    .filter((m) => onlineEntitled || m.id === currentModel)
    .filter((m) => !isPaused(m.id) || m.id === currentModel);
}
