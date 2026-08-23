@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\vsdevcmd.bat" -arch=x64 -host_arch=x64 >nul 2>&1
set PATH=%USERPROFILE%\.cargo\bin;%PATH%
cd /d C:\code\workbuddy-account-hub\tauri-app
echo ==== tauri build start %DATE% %TIME% ==== >> C:\code\workbuddy-account-hub\build.log
npm run tauri build >> C:\code\workbuddy-account-hub\build.log 2>&1
echo BUILD_EXIT=%ERRORLEVEL% >> C:\code\workbuddy-account-hub\build.log
