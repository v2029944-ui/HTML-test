; Keep the shell link's target, arguments, AppUserModelID and user-selected icons.
; Only refresh existing links that point to this installation and use its default icon.
!macro AnalizRefreshIcon shortcut
  ${If} ${FileExists} "${shortcut}"
    !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLinkW} r0 ""
    ${If} $0 P<> 0
      ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
      ${If} $1 P<> 0
        ${IPersistFile::Load} $1 '(w "${shortcut}",${STGM_READWRITE}).r3'
        ${If} $3 = 0
          ; Validate the target on this very object before editing its icon.
          ${IShellLink::GetPath} $0 '(w .r2,${NSIS_MAX_STRLEN},0,0).r3'
          ${If} $3 = 0
          ${AndIf} $2 == "$INSTDIR\${MAINBINARYNAME}.exe"
            ${IShellLink::GetIconLocation} $0 '(w .r2,${NSIS_MAX_STRLEN},.r4).r3'
            ${If} $3 >= 0
              ${If} $2 == ""
              ${OrIf} $2 == "$INSTDIR\${MAINBINARYNAME}.exe"
                ${If} $4 = 0
                  ${IShellLink::SetIconLocation} $0 '(w "$INSTDIR\analysis-bars-blue.ico",0).r3'
                  ${If} $3 = 0
                    ${IPersistFile::Save} $1 '(w "${shortcut}",1).r3'
                    ${If} $3 = 0
                      System::Call 'shell32::SHChangeNotify(i 0x2000, i 5, w "${shortcut}", p 0)'
                    ${EndIf}
                  ${EndIf}
                ${EndIf}
              ${EndIf}
            ${EndIf}
          ${EndIf}
        ${EndIf}
        ${IUnknown::Release} $1 ""
      ${EndIf}
      ${IUnknown::Release} $0 ""
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  !if "${STARTMENUFOLDER}" != ""
    !insertmacro AnalizRefreshIcon "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    !insertmacro AnalizRefreshIcon "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
  !insertmacro AnalizRefreshIcon "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro AnalizRefreshIcon "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\${PRODUCTNAME}.lnk"
  System::Call 'shell32::SHChangeNotify(i 0x2000, i 5, w "$INSTDIR\${MAINBINARYNAME}.exe", p 0)'
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
