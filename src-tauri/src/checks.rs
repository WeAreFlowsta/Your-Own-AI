//! Checks after edits - the project's own check command (typecheck, lint,
//! cargo check, ruff, go vet) run when the agent thinks a turn is done,
//! with failures fed straight back so it fixes them before finishing.
//!
//! Built on the Build agent's Stop hook: a global hook under the agent's
//! home (`~/.your-own-ai-build/hooks/`) runs a small script that detects
//! the check command for the session's folder, runs it, and on failure
//! exits 2 with the errors on stderr - the agent's stop gate turns that
//! into "keep working: <errors>". At most two such rounds per turn (the
//! script counts them by session; the agent itself caps at eight). Every
//! run writes `your-own-ai-checks.last.json`, which the rail reads to say
//! "Checks: passed" or "Checks: failed" honestly - the hook's own output
//! never reaches the app.
//!
//! The hook files are (re)written on every agent start, so an app update
//! updates the script; nothing is installed into the project.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Bump when the script or hook JSON changes; written into the files so a
/// stale copy is recognized and replaced.
const CHECKS_VERSION: &str = "1";
const HOOK_JSON: &str = "your-own-ai-checks.json";
const SCRIPT_SH: &str = "your-own-ai-checks.sh";
const SCRIPT_PS1: &str = "your-own-ai-checks.ps1";
pub const LAST_JSON: &str = "your-own-ai-checks.last.json";

fn hooks_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let home = app.path().home_dir().map_err(|e| format!("cannot resolve home dir: {e}"))?;
    let dir = home.join(".your-own-ai-build").join("hooks");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create hooks folder: {e}"))?;
    Ok(dir)
}

/// POSIX sh. Reads the hook envelope on stdin (cwd, session id, whether
/// this Stop follows an earlier block), picks the project's check command,
/// runs it with a time limit, records the outcome, blocks on failure.
const SCRIPT_SH_BODY: &str = r#"#!/bin/sh
# Your Own AI - checks after edits (version __VERSION__). Rewritten by the app; do not edit.
IN=$(cat)
field() { printf '%s' "$IN" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p" | head -n 1; }
CWD=$(field cwd)
SID=$(field session_id)
ACTIVE=$(printf '%s' "$IN" | grep -o '"stopHookActive":true' | head -n 1)
HOOKS_DIR=$(cd "$(dirname "$0")" && pwd)
LAST="$HOOKS_DIR/your-own-ai-checks.last.json"
[ -n "$CWD" ] && [ -d "$CWD" ] || exit 0
cd "$CWD" || exit 0
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g' | awk '{ printf "%s\\n", $0 }' | sed 's/\\n$//'; }
write_last() {
  printf '{"version":"__VERSION__","folder":"%s","status":"%s","command":"%s","summary":"%s","at":%s}\n' \
    "$(json_escape "$CWD")" "$1" "$(json_escape "$2")" "$(json_escape "$3")" "$(date +%s)" > "$LAST"
}
CMD=""
if [ -f package.json ]; then
  for s in typecheck check lint; do
    if grep -q "\"$s\"[[:space:]]*:" package.json; then CMD="npm run --silent $s"; break; fi
  done
elif [ -f Cargo.toml ]; then CMD="cargo check -q --message-format short"
elif [ -f go.mod ]; then CMD="go vet ./..."
elif [ -f pyproject.toml ] || [ -f ruff.toml ]; then command -v ruff >/dev/null 2>&1 && CMD="ruff check ."
fi
if [ -z "$CMD" ]; then write_last none "" ""; exit 0; fi
# Rounds: a second failure after the agent already tried once is reported, not blocked again.
ROUNDS_FILE="$HOOKS_DIR/.checks-rounds-$(printf '%s' "$SID" | tr -c 'A-Za-z0-9' '_')"
ROUNDS=0
if [ -n "$ACTIVE" ] && [ -f "$ROUNDS_FILE" ]; then ROUNDS=$(cat "$ROUNDS_FILE" 2>/dev/null || echo 0); fi
[ -z "$ACTIVE" ] && rm -f "$ROUNDS_FILE"
OUT=$( (command -v timeout >/dev/null 2>&1 && timeout 60 sh -c "$CMD" || sh -c "$CMD") 2>&1 )
CODE=$?
if [ "$CODE" -eq 0 ]; then
  write_last passed "$CMD" "$(printf '%s' "$OUT" | tail -n 3)"
  rm -f "$ROUNDS_FILE"
  exit 0
fi
TAIL=$(printf '%s' "$OUT" | grep -v '^$' | awk '!seen[$0]++' | tail -n 60)
write_last failed "$CMD" "$(printf '%s' "$TAIL" | tail -n 25)"
if [ "$ROUNDS" -ge 2 ]; then exit 0; fi
echo $((ROUNDS + 1)) > "$ROUNDS_FILE"
printf 'The project checks failed (`%s`). Fix these before finishing:\n%s\n' "$CMD" "$TAIL" >&2
exit 2
"#;

/// PowerShell twin of the sh script.
const SCRIPT_PS1_BODY: &str = r#"# Your Own AI - checks after edits (version __VERSION__). Rewritten by the app; do not edit.
$ErrorActionPreference = 'Continue'
$inText = [Console]::In.ReadToEnd()
try { $in = $inText | ConvertFrom-Json } catch { exit 0 }
$cwd = [string]$in.cwd
$sid = [string]$in.session_id
$active = [bool]$in.stopHookActive
$hooksDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$last = Join-Path $hooksDir 'your-own-ai-checks.last.json'
if (-not $cwd -or -not (Test-Path -LiteralPath $cwd)) { exit 0 }
Set-Location -LiteralPath $cwd
function Write-Last($status, $command, $summary) {
  $o = @{ version = '__VERSION__'; folder = $cwd; status = $status; command = $command; summary = $summary; at = [int][double]::Parse((Get-Date -UFormat %s)) }
  $o | ConvertTo-Json -Compress | Set-Content -LiteralPath $last -Encoding UTF8
}
$cmd = ''
if (Test-Path package.json) {
  try { $pkg = Get-Content package.json -Raw | ConvertFrom-Json } catch { $pkg = $null }
  foreach ($s in @('typecheck','check','lint')) { if ($pkg -and $pkg.scripts -and $pkg.scripts.PSObject.Properties[$s]) { $cmd = "npm run --silent $s"; break } }
} elseif (Test-Path Cargo.toml) { $cmd = 'cargo check -q --message-format short' }
elseif (Test-Path go.mod) { $cmd = 'go vet ./...' }
elseif ((Test-Path pyproject.toml) -or (Test-Path ruff.toml)) { if (Get-Command ruff -ErrorAction SilentlyContinue) { $cmd = 'ruff check .' } }
if (-not $cmd) { Write-Last 'none' '' ''; exit 0 }
$safeSid = ($sid -replace '[^A-Za-z0-9]', '_')
$roundsFile = Join-Path $hooksDir ".checks-rounds-$safeSid"
$rounds = 0
if ($active -and (Test-Path -LiteralPath $roundsFile)) { try { $rounds = [int](Get-Content -LiteralPath $roundsFile) } catch { $rounds = 0 } }
if (-not $active) { Remove-Item -LiteralPath $roundsFile -ErrorAction SilentlyContinue }
$out = ''
$code = 1
try {
  $p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/d','/c', $cmd) -NoNewWindow -PassThru -RedirectStandardOutput "$env:TEMP\yoai-checks-out.txt" -RedirectStandardError "$env:TEMP\yoai-checks-err.txt"
  if (-not $p.WaitForExit(60000)) { $p.Kill(); $out = 'Timed out after 60 s'; $code = 124 } else { $code = $p.ExitCode; $out = ((Get-Content "$env:TEMP\yoai-checks-out.txt" -Raw) + "`n" + (Get-Content "$env:TEMP\yoai-checks-err.txt" -Raw)) }
} catch { $out = "$_"; $code = 1 }
$lines = ($out -split "`r?`n") | Where-Object { $_ -ne '' }
if ($code -eq 0) { Write-Last 'passed' $cmd (($lines | Select-Object -Last 3) -join "`n"); Remove-Item -LiteralPath $roundsFile -ErrorAction SilentlyContinue; exit 0 }
$tail = ($lines | Select-Object -Last 60) -join "`n"
Write-Last 'failed' $cmd (($lines | Select-Object -Last 25) -join "`n")
if ($rounds -ge 2) { exit 0 }
Set-Content -LiteralPath $roundsFile -Value ($rounds + 1)
[Console]::Error.WriteLine("The project checks failed (``$cmd``). Fix these before finishing:`n$tail")
exit 2
"#;

fn hook_json(command: &str) -> String {
    serde_json::json!({
        "_your_own_ai": { "version": CHECKS_VERSION },
        "hooks": {
            "Stop": [ { "hooks": [ { "type": "command", "command": command, "timeout": 90 } ] } ]
        }
    })
    .to_string()
}

fn write_if_changed(path: &std::path::Path, content: &str) -> Result<bool, String> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
        return Ok(false);
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(true)
}

/// (Re)write the hook files. Called before every agent start; cheap when
/// nothing changed.
pub(crate) fn install_check_hooks(app: &AppHandle) -> Result<(), String> {
    let dir = hooks_dir(app)?;
    let sh = SCRIPT_SH_BODY.replace("__VERSION__", CHECKS_VERSION);
    let ps1 = SCRIPT_PS1_BODY.replace("__VERSION__", CHECKS_VERSION);
    write_if_changed(&dir.join(SCRIPT_SH), &sh)?;
    write_if_changed(&dir.join(SCRIPT_PS1), &ps1)?;
    // `$HOME` / `$USERPROFILE` in the command makes the runner treat it as a
    // shell command (sh -c on Unix, PowerShell on Windows).
    let command = if cfg!(windows) {
        format!("powershell -NoProfile -ExecutionPolicy Bypass -File \"$USERPROFILE/.your-own-ai-build/hooks/{SCRIPT_PS1}\"")
    } else {
        format!("sh \"$HOME/.your-own-ai-build/hooks/{SCRIPT_SH}\"")
    };
    if write_if_changed(&dir.join(HOOK_JSON), &hook_json(&command))? {
        log::info!("[checks] hook installed in {}", dir.display());
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChecksResult {
    pub folder: String,
    /// "passed" | "failed" | "none"
    pub status: String,
    pub command: String,
    pub summary: String,
    pub at: u64,
}

/// The last check outcome for a folder, if the hook has run there.
#[tauri::command]
pub async fn project_checks_last(app: AppHandle, folder: String) -> Result<Option<ChecksResult>, String> {
    let path = hooks_dir(&app)?.join(LAST_JSON);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(None) };
    let Ok(r) = serde_json::from_str::<ChecksResult>(&text) else { return Ok(None) };
    if r.folder.trim_end_matches(['/', '\\']) != folder.trim_end_matches(['/', '\\']) {
        return Ok(None);
    }
    Ok(Some(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_json_is_a_stop_hook_with_a_real_timeout() {
        let j: serde_json::Value = serde_json::from_str(&hook_json("sh x")).unwrap();
        let h = &j["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(h["type"], "command");
        assert_eq!(h["command"], "sh x");
        assert!(h["timeout"].as_u64().unwrap() >= 60);
    }

    #[test]
    fn scripts_carry_the_version() {
        assert!(SCRIPT_SH_BODY.contains("__VERSION__"));
        assert!(SCRIPT_PS1_BODY.contains("__VERSION__"));
        assert!(!SCRIPT_SH_BODY.replace("__VERSION__", CHECKS_VERSION).contains("__VERSION__"));
    }
}
