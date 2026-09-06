/**
 * Who receives files dropped on the window. Tauri delivers a drop to every
 * listener, so the chat input (always mounted behind a dialog) would attach
 * a file that was meant for the dialog's Knowledge tab. A section that wants
 * the drops claims them while it is on screen; the chat input stands aside.
 */
let claimedBy: string | null = null;

export function claimFileDrops(owner: string): void {
  claimedBy = owner;
}

export function releaseFileDrops(owner: string): void {
  if (claimedBy === owner) claimedBy = null;
}

export function fileDropsClaimed(): boolean {
  return claimedBy !== null;
}
