; Your Own AI 0.9.0 and earlier installed the app as app.exe; from 0.9.1 it is
; your-own-ai.exe. An install over an earlier version removes the old file.
!macro NSIS_HOOK_POSTINSTALL
  Delete "$INSTDIR\app.exe"
!macroend
