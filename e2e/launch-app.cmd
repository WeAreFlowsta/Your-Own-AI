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
"%HERE%..\src-tauri\target\debug\app.exe" %*
