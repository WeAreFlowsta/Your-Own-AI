; Your Own AI 0.9.0 and earlier installed the app as app.exe and its engine as
; llama-server.exe; from 0.9.1 they are your-own-ai.exe and
; yourowai-llama-server.exe. An install over an earlier version removes the
; old files.
!macro NSIS_HOOK_POSTINSTALL
  Delete "$INSTDIR\app.exe"
  Delete "$INSTDIR\llama-server.exe"
!macroend
