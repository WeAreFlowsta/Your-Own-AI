/**
 * The live turns, mirrored into Qwik: the root layout provides it and
 * refreshes it on every TURNS_EVENT; pages and the activity tray read it.
 */
import { createContextId, useContext, useContextProvider, useStore, useVisibleTask$ } from "@builder.io/qwik";
import { allTurns, TURNS_EVENT, type Turn } from "../utils/turns";

export interface TurnsState {
  turns: Turn[];
}

export const TurnsContext = createContextId<TurnsState>("app.turns");

export function useProvideTurns(): TurnsState {
  const store = useStore<TurnsState>({ turns: [] });
  useContextProvider(TurnsContext, store);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const refresh = () => { store.turns = allTurns().map((t) => ({ ...t, user: { ...t.user }, assistant: { ...t.assistant } })); };
    refresh();
    window.addEventListener(TURNS_EVENT, refresh);
    cleanup(() => window.removeEventListener(TURNS_EVENT, refresh));
  });
  return store;
}

export const useTurns = () => useContext(TurnsContext);
