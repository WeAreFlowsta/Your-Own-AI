param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$FilePath
)

$ErrorActionPreference = "Stop"

# Mirror all output to a log file so CI can dump it on failure
# (tauri-bundler captures stdout/stderr but drops them on non-zero exit).
$logDir = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$logFile = Join-Path $logDir "sign-windows.log"
Start-Transcript -Path $logFile -Append -Force | Out-Null

# Fail fast if Tauri didn't substitute the signCommand placeholder.
if ($FilePath -match '^%\d+$' -or $FilePath -eq '%1') {
    throw "signCommand placeholder was passed literally: '$FilePath'. Tauri did not substitute the file path."
}

foreach ($name in 'ESIGNER_USERNAME','ESIGNER_PASSWORD','ESIGNER_CREDENTIAL_ID','ESIGNER_TOTP_SECRET','CODE_SIGN_TOOL_DIR') {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        throw "Environment variable $name is not set"
    }
}

$jar = Get-ChildItem -Path (Join-Path $env:CODE_SIGN_TOOL_DIR 'jar') -Filter 'code_sign_tool-*.jar' | Select-Object -First 1
if (-not $jar) { throw "CodeSignTool jar not found under $env:CODE_SIGN_TOOL_DIR\jar" }

if (-not (Test-Path -LiteralPath $FilePath)) {
    throw "File to sign does not exist: $FilePath"
}

# Skip extensions Authenticode / CodeSignTool can't sign. NSIS occasionally
# asks us to sign its own .tmp scratch files during plugin packing — without
# this guard, CodeSignTool errors and the build log gets noisy (NSIS itself
# tolerates the failure, but it masks real errors).
$signableExt = @('.exe', '.dll', '.msi', '.msix', '.appx', '.cab', '.ocx', '.sys', '.cat')
$ext = [System.IO.Path]::GetExtension($FilePath).ToLowerInvariant()
if (-not $signableExt.Contains($ext)) {
    Write-Host "Skipping unsignable file ($ext): $FilePath"
    exit 0
}

# Tauri passes the file as a path relative to its own CWD. We change CWD
# below to load CodeSignTool's conf/, so resolve to absolute first or
# CodeSignTool will look in its own directory and fail with
# "Invalid input file path".
$FilePath = (Resolve-Path -LiteralPath $FilePath).Path

Write-Host "Signing $FilePath"

# CodeSignTool reads conf/code_sign_tool.properties relative to CWD, so run from its root dir.
Push-Location $env:CODE_SIGN_TOOL_DIR
try {
    $output = & java -jar $jar.FullName sign `
        "-username=$env:ESIGNER_USERNAME" `
        "-password=$env:ESIGNER_PASSWORD" `
        "-credential_id=$env:ESIGNER_CREDENTIAL_ID" `
        "-totp_secret=$env:ESIGNER_TOTP_SECRET" `
        "-input_file_path=$FilePath" `
        -override 2>&1 | Out-String
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}

Write-Host $output

# CodeSignTool sometimes prints "Error: ..." on stdout while exiting zero, so check both.
if ($code -ne 0 -or $output -match '(?m)^Error:') {
    throw "CodeSignTool failed (exit=$code) for $FilePath"
}
Write-Host "Signed $FilePath"
