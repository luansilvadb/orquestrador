@rem Runner do cargo (target.x86_64-pc-windows-msvc.runner): poe as DLLs do FFmpeg no PATH e executa o binario.
@set "PATH=%~dp0..\ferramentas\ffmpeg\bin;%PATH%"
@%*
@exit /b %ERRORLEVEL%
