; Дополнения к установщику Tauri (bundle > windows > nsis > installerHooks).

!macro NSIS_HOOK_PREINSTALL
  ; Пункт меню правой кнопки был до версии 1.4. Его библиотеку может держать открытой Проводник:
  ; занятый файл нельзя удалить, но можно переименовать — удалится после перезагрузки.
  IfFileExists "$INSTDIR\ObshayaShell.dll" 0 +3
    Delete "$INSTDIR\ObshayaShell.dll.old"
    Rename "$INSTDIR\ObshayaShell.dll" "$INSTDIR\ObshayaShell.dll.old"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Delete /REBOOTOK "$INSTDIR\ObshayaShell.dll.old"
  Delete "$INSTDIR\ObshayaShell.msix"
  Delete "$INSTDIR\ObshayaShell.cer"
  ; Сам установщик и его подпись (если лежит рядом) — программа отдаст их устройствам семьи
  ; со старой версией (обновление без сервера). Без подписи другие устройства его не примут.
  CreateDirectory "$APPDATA\ObshayaPapka\update"
  Delete "$APPDATA\ObshayaPapka\update\installer.exe.sig"
  CopyFiles /SILENT "$EXEPATH" "$APPDATA\ObshayaPapka\update\installer.exe"
  IfFileExists "$EXEPATH.sig" 0 +2
    CopyFiles /SILENT "$EXEPATH.sig" "$APPDATA\ObshayaPapka\update\installer.exe.sig"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Пока программа на месте: убрать пункт меню, ярлыки, закрепление и автозапуск.
  ; Файлы в общей папке и настройки остаются.
  ${If} $UpdateMode <> 1
    nsExec::Exec '"$INSTDIR\ObshayaPapka.exe" --uninstall'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete /REBOOTOK "$INSTDIR\ObshayaShell.dll"
  Delete /REBOOTOK "$INSTDIR\ObshayaShell.dll.old"
  RMDir /REBOOTOK "$INSTDIR"
!macroend
