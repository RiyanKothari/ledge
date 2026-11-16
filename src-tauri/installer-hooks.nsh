; Ledge installer hooks: never leave anyone without a taskbar.

!macro NSIS_HOOK_PREINSTALL
  ; Upgrading: close the running Ledge and put the Windows taskbar back first.
  ; The new Ledge hides it again when it starts.
  nsExec::Exec 'taskkill /F /IM ledge.exe'
  Sleep 800
  IfFileExists "$INSTDIR\ledge.exe" 0 +2
    ExecWait '"$INSTDIR\ledge.exe" --restore-taskbar'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Uninstalling: close Ledge, restore the Windows taskbar exactly as it was,
  ; and stop Ledge starting at sign-in.
  nsExec::Exec 'taskkill /F /IM ledge.exe'
  Sleep 800
  IfFileExists "$INSTDIR\ledge.exe" 0 +2
    ExecWait '"$INSTDIR\ledge.exe" --restore-taskbar'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Ledge"
!macroend
