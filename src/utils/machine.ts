import { invoke } from "@tauri-apps/api/core";

/**
 * Background work asks the whole-machine check (src-tauri/src/machine.rs)
 * before each unit. Heavy work runs only when other programs leave the
 * processor alone, memory is not short, the computer is on mains power and
 * the person has been away from the keyboard for a few minutes; light work
 * only when memory is not short. While the answer is "not now" this waits,
 * looking again every 30 s, and the Activity tray says what waits.
 *
 * A backend without the check (an older build) answers by throwing: the
 * work then goes ahead, as it did before.
 */
export async function waitForCalm(job: string, heavy = true): Promise<void> {
  for (;;) {
    let ok = true;
    try {
      ok = await invoke<boolean>("machine_may_run", { heavy, job });
    } catch {
      ok = true;
    }
    if (ok) return;
    await new Promise((r) => setTimeout(r, 30_000));
  }
}

/** A job that finished or stopped no longer shows as waiting. */
export function machineDone(job: string): void {
  void invoke("machine_done", { job }).catch(() => {});
}

export interface MachineStatus {
  reading: { others_cpu_pct: number | null; memory_short: boolean; on_battery: boolean; away_secs: number | null } | null;
  /** [job, why] for each job waiting for the computer. */
  held: [string, string][];
}

export async function machineStatus(): Promise<MachineStatus | null> {
  try {
    return await invoke<MachineStatus>("machine_status");
  } catch {
    return null;
  }
}
