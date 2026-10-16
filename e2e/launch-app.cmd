@echo off
rem Windows twin of launch-app.sh: the e2e build in an isolated profile.
setlocal
set "HERE=%~dp0"
if "%YOAI_E2E_HOME%"=="" (set "YOAI_E2E_HOME=%HERE%profile")
set "USERPROFILE=%YOAI_E2E_HOME%"
set "APPDATA=%YOAI_E2E_HOME%\AppData\Roaming"
set "LOCALAPPDATA=%YOAI_E2E_HOME%\AppData\Local"
if not exist "%LOCALAPPDATA%" mkdir "%LOCALAPPDATA%"
if not exist "%APPDATA%" mkdir "%APPDATA%"
rem YOAI_E2E_ONLINE=1: the staging proxy and an unanswered Vault port (see launch-app.sh).
if not "%YOAI_E2E_ONLINE%"=="" (
  if "%YOAI_PROXY_URL%"=="" set "YOAI_PROXY_URL=https://yoai-model-proxy-staging-386500392150.us-central1.run.app"
  if "%YOAI_VAULT_PORT%"=="" set "YOAI_VAULT_PORT=27990"
)
"%HERE%..\src-tauri\target\debug\app.exe" %*
