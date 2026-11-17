; An update is started by the running app, which then exits. Windows can keep
; hold of the program file a moment longer, and the installer then stops at
; "Error opening file for writing" with Abort / Retry / Ignore. In an update,
; wait until the file can be opened for writing, up to 15 seconds, before the
; installer goes on. A file that is free passes at once; an install by hand
; is not held up (the installer's own check deals with a running app).
!macro WaitForProgramFile
  ${If} $UpdateMode = 1
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    Push $R8
    Push $R9
    StrCpy $R8 0
    ${Do}
      ClearErrors
      FileOpen $R9 "$INSTDIR\${MAINBINARYNAME}.exe" a
      ${IfNot} ${Errors}
        FileClose $R9
        ${Break}
      ${EndIf}
      IntOp $R8 $R8 + 1
      ${If} $R8 >= 60
        ${Break}
      ${EndIf}
      Sleep 250
    ${Loop}
    Pop $R9
    Pop $R8
  ${EndIf}
!macroend
