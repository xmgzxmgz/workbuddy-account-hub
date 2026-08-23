#!/usr/bin/env bash
# 在 Git Bash 中初始化 MSVC 环境并构建 Tauri (release)
set -e

VS="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
MSVC_VER=$(ls "$VS/VC/Tools/MSVC" | head -1)
MSVC_BIN="$VS/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64"
SDK="C:/Program Files (x86)/Windows Kits/10"
SDK_VER=$(ls "$SDK/Lib" | head -1)

export PATH="$HOME/.cargo/bin:$MSVC_BIN:$PATH"
export INCLUDE="$VS/VC/Tools/MSVC/$MSVC_VER/include;$VS/VC/Tools/MSVC/$MSVC_VER/ATLMFC/include;$SDK/Include/$SDK_VER/um;$SDK/Include/$SDK_VER/shared;$SDK/Include/$SDK_VER/winrt;$SDK/Include/$SDK_VER/ucrt"
export LIB="$VS/VC/Tools/MSVC/$MSVC_VER/lib/x64;$VS/VC/Tools/MSVC/$MSVC_VER/ATLMFC/lib/x64;$SDK/Lib/$SDK_VER/um/x64;$SDK/Lib/$SDK_VER/ucrt/x64"

cd "/c/code/workbuddy-account-hub/tauri-app"
echo "==== tauri build (release) start $(date) ====" >> /c/code/workbuddy-account-hub/build.log
npm run tauri build >> /c/code/workbuddy-account-hub/build.log 2>&1
echo "BUILD_EXIT=$?" >> /c/code/workbuddy-account-hub/build.log
