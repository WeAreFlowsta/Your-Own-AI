/**
 * Line diff for the agent's edit cards - the permission card (before the
 * user allows an edit) and the work rail's completed step share it. Real
 * ordered hunks with context, not a set-membership preview.
 */

export interface DiffLine {
  /** "+" added, "-" removed, " " unchanged context, "…" gap between hunks. */
  sign: "+" | "-" | " " | "…";
  text: string;
}

export interface LineDiff {
  lines: DiffLine[];
  added: number;
  removed: number;
}

const CONTEXT = 2; // unchanged lines kept around each hunk
const LCS_LIMIT = 400; // beyond this (per side) fall back to replace-all

/** Longest-common-subsequence keep-flags for old vs new line arrays. */
function lcsKeep(a: string[], b: string[]): [boolean[], boolean[]] {
  const n = a.length;
  const m = b.length;
  // One row at a time keeps memory at O(m); n*m is capped by LCS_LIMIT^2.
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const keepA = new Array(n).fill(false);
  const keepB = new Array(m).fill(false);
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      keepA[i] = true;
      keepB[j] = true;
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return [keepA, keepB];
}

/** Full ordered diff rows (before context folding). */
function rawRows(oldLines: string[], newLines: string[]): DiffLine[] {
  // Trim the common prefix/suffix first - most edits are one contiguous
  // change, and this keeps the LCS window small.
  let start = 0;
  while (
    start < oldLines.length &&
    start < newLines.length &&
    oldLines[start] === newLines[start]
  ) {
    start++;
  }
  let endOld = oldLines.length;
  let endNew = newLines.length;
  while (
    endOld > start &&
    endNew > start &&
    oldLines[endOld - 1] === newLines[endNew - 1]
  ) {
    endOld--;
    endNew--;
  }
  const midOld = oldLines.slice(start, endOld);
  const midNew = newLines.slice(start, endNew);

  const rows: DiffLine[] = oldLines
    .slice(0, start)
    .map((text) => ({ sign: " " as const, text }));

  if (midOld.length > LCS_LIMIT || midNew.length > LCS_LIMIT) {
    // Too big for a precise diff - show the middle as replaced wholesale.
    rows.push(...midOld.map((text) => ({ sign: "-" as const, text })));
    rows.push(...midNew.map((text) => ({ sign: "+" as const, text })));
  } else {
    const [keepA, keepB] = lcsKeep(midOld, midNew);
    let i = 0;
    let j = 0;
    while (i < midOld.length || j < midNew.length) {
      if (i < midOld.length && !keepA[i]) {
        rows.push({ sign: "-", text: midOld[i++] });
      } else if (j < midNew.length && !keepB[j]) {
        rows.push({ sign: "+", text: midNew[j++] });
      } else if (i < midOld.length && j < midNew.length) {
        rows.push({ sign: " ", text: midOld[i] });
        i++;
        j++;
      } else {
        break;
      }
    }
  }

  rows.push(
    ...oldLines.slice(endOld).map((text) => ({ sign: " " as const, text })),
  );
  return rows;
}

/** Diff old → new, folding long unchanged stretches into "…" gap rows. */
export function computeLineDiff(oldText: string | null, newText: string): LineDiff {
  const newLines = newText.split("\n");
  if (oldText == null) {
    // New file: everything is an addition.
    return {
      lines: newLines.map((text) => ({ sign: "+" as const, text })),
      added: newLines.length,
      removed: 0,
    };
  }
  const rows = rawRows(oldText.split("\n"), newLines);
  const added = rows.filter((r) => r.sign === "+").length;
  const removed = rows.filter((r) => r.sign === "-").length;

  // Fold unchanged runs beyond CONTEXT lines on either side of a change.
  const nearChange = new Array(rows.length).fill(false);
  rows.forEach((r, idx) => {
    if (r.sign === "+" || r.sign === "-") {
      for (
        let k = Math.max(0, idx - CONTEXT);
        k <= Math.min(rows.length - 1, idx + CONTEXT);
        k++
      ) {
        nearChange[k] = true;
      }
    }
  });
  const lines: DiffLine[] = [];
  let gapOpen = false;
  rows.forEach((r, idx) => {
    if (r.sign === " " && !nearChange[idx]) {
      if (!gapOpen) {
        lines.push({ sign: "…", text: "" });
        gapOpen = true;
      }
      return;
    }
    gapOpen = false;
    lines.push(r);
  });
  return { lines, added, removed };
}
