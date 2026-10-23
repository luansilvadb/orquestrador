!macro ORQ_REGISTER_EXTENSION EXT
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "Orquestrador.File" ""
  WriteRegStr HKCU "Software\Classes\Applications\orquestrador.exe\SupportedTypes" ".${EXT}" ""
!macroend

!macro ORQ_REMOVE_EXTENSION EXT
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "Orquestrador.File"
  DeleteRegValue HKCU "Software\Classes\Applications\orquestrador.exe\SupportedTypes" ".${EXT}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\Orquestrador.File" "" "Arquivo do Orquestrador"
  WriteRegStr HKCU "Software\Classes\Orquestrador.File\DefaultIcon" "" "$INSTDIR\orquestrador.exe,0"
  WriteRegStr HKCU "Software\Classes\Orquestrador.File\shell\open\command" "" '"$INSTDIR\orquestrador.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\orquestrador.exe" "FriendlyAppName" "Orquestrador"
  WriteRegStr HKCU "Software\Classes\Applications\orquestrador.exe\DefaultIcon" "" "$INSTDIR\orquestrador.exe,0"
  WriteRegStr HKCU "Software\Classes\Applications\orquestrador.exe\shell\open\command" "" '"$INSTDIR\orquestrador.exe" "%1"'

  WriteRegStr HKCU "Software\Classes\*\shell\Orquestrador" "" "Abrir com Orquestrador"
  WriteRegStr HKCU "Software\Classes\*\shell\Orquestrador" "Icon" "$INSTDIR\orquestrador.exe,0"
  WriteRegStr HKCU "Software\Classes\*\shell\Orquestrador\command" "" '"$INSTDIR\orquestrador.exe" "%1"'

  WriteRegStr HKCU "Software\Classes\Directory\shell\Orquestrador" "" "Abrir pasta no Orquestrador"
  WriteRegStr HKCU "Software\Classes\Directory\shell\Orquestrador" "Icon" "$INSTDIR\orquestrador.exe,0"
  WriteRegStr HKCU "Software\Classes\Directory\shell\Orquestrador\command" "" '"$INSTDIR\orquestrador.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\Orquestrador" "" "Abrir pasta no Orquestrador"
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\Orquestrador" "Icon" "$INSTDIR\orquestrador.exe,0"
  WriteRegStr HKCU "Software\Classes\Directory\Background\shell\Orquestrador\command" "" '"$INSTDIR\orquestrador.exe" "%V"'

  !insertmacro ORQ_REGISTER_EXTENSION txt
  !insertmacro ORQ_REGISTER_EXTENSION md
  !insertmacro ORQ_REGISTER_EXTENSION json
  !insertmacro ORQ_REGISTER_EXTENSION jsonc
  !insertmacro ORQ_REGISTER_EXTENSION yaml
  !insertmacro ORQ_REGISTER_EXTENSION yml
  !insertmacro ORQ_REGISTER_EXTENSION toml
  !insertmacro ORQ_REGISTER_EXTENSION ini
  !insertmacro ORQ_REGISTER_EXTENSION cfg
  !insertmacro ORQ_REGISTER_EXTENSION conf
  !insertmacro ORQ_REGISTER_EXTENSION env
  !insertmacro ORQ_REGISTER_EXTENSION xml
  !insertmacro ORQ_REGISTER_EXTENSION csv
  !insertmacro ORQ_REGISTER_EXTENSION log
  !insertmacro ORQ_REGISTER_EXTENSION js
  !insertmacro ORQ_REGISTER_EXTENSION jsx
  !insertmacro ORQ_REGISTER_EXTENSION mjs
  !insertmacro ORQ_REGISTER_EXTENSION cjs
  !insertmacro ORQ_REGISTER_EXTENSION ts
  !insertmacro ORQ_REGISTER_EXTENSION tsx
  !insertmacro ORQ_REGISTER_EXTENSION css
  !insertmacro ORQ_REGISTER_EXTENSION scss
  !insertmacro ORQ_REGISTER_EXTENSION less
  !insertmacro ORQ_REGISTER_EXTENSION html
  !insertmacro ORQ_REGISTER_EXTENSION htm
  !insertmacro ORQ_REGISTER_EXTENSION vue
  !insertmacro ORQ_REGISTER_EXTENSION py
  !insertmacro ORQ_REGISTER_EXTENSION rs
  !insertmacro ORQ_REGISTER_EXTENSION go
  !insertmacro ORQ_REGISTER_EXTENSION java
  !insertmacro ORQ_REGISTER_EXTENSION kt
  !insertmacro ORQ_REGISTER_EXTENSION c
  !insertmacro ORQ_REGISTER_EXTENSION h
  !insertmacro ORQ_REGISTER_EXTENSION cpp
  !insertmacro ORQ_REGISTER_EXTENSION hpp
  !insertmacro ORQ_REGISTER_EXTENSION cs
  !insertmacro ORQ_REGISTER_EXTENSION rb
  !insertmacro ORQ_REGISTER_EXTENSION php
  !insertmacro ORQ_REGISTER_EXTENSION sh
  !insertmacro ORQ_REGISTER_EXTENSION ps1
  !insertmacro ORQ_REGISTER_EXTENSION bat
  !insertmacro ORQ_REGISTER_EXTENSION cmd
  !insertmacro ORQ_REGISTER_EXTENSION sql
  !insertmacro ORQ_REGISTER_EXTENSION gitignore
  !insertmacro ORQ_REGISTER_EXTENSION editorconfig
  !insertmacro ORQ_REGISTER_EXTENSION properties
  !insertmacro ORQ_REGISTER_EXTENSION lock
  !insertmacro ORQ_REGISTER_EXTENSION dockerfile
  !insertmacro ORQ_REGISTER_EXTENSION makefile
  !insertmacro ORQ_REGISTER_EXTENSION swift
  !insertmacro ORQ_REGISTER_EXTENSION dart
  !insertmacro ORQ_REGISTER_EXTENSION scala

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\Orquestrador"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\Orquestrador"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\Orquestrador"
  DeleteRegKey HKCU "Software\Classes\Orquestrador.File"

  !insertmacro ORQ_REMOVE_EXTENSION txt
  !insertmacro ORQ_REMOVE_EXTENSION md
  !insertmacro ORQ_REMOVE_EXTENSION json
  !insertmacro ORQ_REMOVE_EXTENSION jsonc
  !insertmacro ORQ_REMOVE_EXTENSION yaml
  !insertmacro ORQ_REMOVE_EXTENSION yml
  !insertmacro ORQ_REMOVE_EXTENSION toml
  !insertmacro ORQ_REMOVE_EXTENSION ini
  !insertmacro ORQ_REMOVE_EXTENSION cfg
  !insertmacro ORQ_REMOVE_EXTENSION conf
  !insertmacro ORQ_REMOVE_EXTENSION env
  !insertmacro ORQ_REMOVE_EXTENSION xml
  !insertmacro ORQ_REMOVE_EXTENSION csv
  !insertmacro ORQ_REMOVE_EXTENSION log
  !insertmacro ORQ_REMOVE_EXTENSION js
  !insertmacro ORQ_REMOVE_EXTENSION jsx
  !insertmacro ORQ_REMOVE_EXTENSION mjs
  !insertmacro ORQ_REMOVE_EXTENSION cjs
  !insertmacro ORQ_REMOVE_EXTENSION ts
  !insertmacro ORQ_REMOVE_EXTENSION tsx
  !insertmacro ORQ_REMOVE_EXTENSION css
  !insertmacro ORQ_REMOVE_EXTENSION scss
  !insertmacro ORQ_REMOVE_EXTENSION less
  !insertmacro ORQ_REMOVE_EXTENSION html
  !insertmacro ORQ_REMOVE_EXTENSION htm
  !insertmacro ORQ_REMOVE_EXTENSION vue
  !insertmacro ORQ_REMOVE_EXTENSION py
  !insertmacro ORQ_REMOVE_EXTENSION rs
  !insertmacro ORQ_REMOVE_EXTENSION go
  !insertmacro ORQ_REMOVE_EXTENSION java
  !insertmacro ORQ_REMOVE_EXTENSION kt
  !insertmacro ORQ_REMOVE_EXTENSION c
  !insertmacro ORQ_REMOVE_EXTENSION h
  !insertmacro ORQ_REMOVE_EXTENSION cpp
  !insertmacro ORQ_REMOVE_EXTENSION hpp
  !insertmacro ORQ_REMOVE_EXTENSION cs
  !insertmacro ORQ_REMOVE_EXTENSION rb
  !insertmacro ORQ_REMOVE_EXTENSION php
  !insertmacro ORQ_REMOVE_EXTENSION sh
  !insertmacro ORQ_REMOVE_EXTENSION ps1
  !insertmacro ORQ_REMOVE_EXTENSION bat
  !insertmacro ORQ_REMOVE_EXTENSION cmd
  !insertmacro ORQ_REMOVE_EXTENSION sql
  !insertmacro ORQ_REMOVE_EXTENSION gitignore
  !insertmacro ORQ_REMOVE_EXTENSION editorconfig
  !insertmacro ORQ_REMOVE_EXTENSION properties
  !insertmacro ORQ_REMOVE_EXTENSION lock
  !insertmacro ORQ_REMOVE_EXTENSION dockerfile
  !insertmacro ORQ_REMOVE_EXTENSION makefile
  !insertmacro ORQ_REMOVE_EXTENSION swift
  !insertmacro ORQ_REMOVE_EXTENSION dart
  !insertmacro ORQ_REMOVE_EXTENSION scala

  DeleteRegKey HKCU "Software\Classes\Applications\orquestrador.exe"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
