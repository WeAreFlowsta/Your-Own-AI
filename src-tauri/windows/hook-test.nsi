; A small installer that runs the update wait the way the real installer
; does, for the CI check in test.yml: the "program file" is held by another
; process for a few seconds, and this must wait for it and then write it.
; Exit code 0: written. 3: the file could not be written after the wait.
Unicode true
!include LogicLib.nsh
!include FileFunc.nsh
OutFile "hook-test.exe"
RequestExecutionLevel user
SilentInstall silent
Var UpdateMode
!define MAINBINARYNAME "hook-target"
!include "hooks.nsh"

Section
  StrCpy $INSTDIR "$EXEDIR\hook-test-inst"
  ${GetParameters} $0
  ${If} $0 == "/UPDATE"
    StrCpy $UpdateMode 1
  ${EndIf}
  !insertmacro NSIS_HOOK_PREINSTALL
  ClearErrors
  FileOpen $1 "$INSTDIR\hook-target.exe" w
  ${If} ${Errors}
    SetErrorLevel 3
  ${Else}
    FileWrite $1 "new"
    FileClose $1
    SetErrorLevel 0
  ${EndIf}
SectionEnd
