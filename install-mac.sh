#!/usr/bin/env bash
# 一键安装 / 升级 WorkBuddy Account Hub (macOS)
#
# 为什么需要它：本项目的 Mac 构建未做 Apple 公证（无 Developer ID 证书），
# 用浏览器下载会被 Gatekeeper 标记 quarantine 并报「已损坏」。
# 本脚本改用 curl 下载（不触发 quarantine），并主动清除隔离属性，
# 这样下载后「直接打开就能用」，不用每次记 xattr 命令。
#
# 用法（二选一）：
#   1) 直接跑远程脚本：
#      bash -c "$(curl -fsSL https://raw.githubusercontent.com/xmgzxmgz/workbuddy-account-hub/main/install-mac.sh)"
#   2) 或下载后本地运行：
#      curl -fsSL https://raw.githubusercontent.com/xmgzxmgz/workbuddy-account-hub/main/install-mac.sh -o install-mac.sh
#      chmod +x install-mac.sh && ./install-mac.sh
set -uo pipefail

REPO="xmgzxmgz/workbuddy-account-hub"
APP_NAME="WorkBuddy Account Hub"

echo "==> 查询 $REPO 最新 release ..."
ASSET_URL=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | python3 -c "import sys,json
d=json.load(sys.stdin)
cands=[x['browser_download_url'] for x in d.get('assets',[]) if x['name'].endswith('.dmg')]
print(cands[0] if cands else '')" 2>/dev/null)

if [ -z "$ASSET_URL" ]; then
  echo "错误：未在最新 release 找到 .dmg 资产（可能 GitHub API 限流，稍后重试即可）" >&2
  exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "==> 下载 $ASSET_URL"
curl -fL "$ASSET_URL" -o "$TMP/app.dmg"

echo "==> 挂载 dmg"
MNT=$(hdiutil attach "$TMP/app.dmg" -noautoopen -nobrowse 2>/dev/null | awk 'END{print $NF}')
if [ -z "$MNT" ] || [ ! -d "$MNT" ]; then
  echo "错误：dmg 挂载失败" >&2
  exit 1
fi

SRC=$(find "$MNT" -maxdepth 2 -name '*.app' | head -1)
if [ -z "$SRC" ]; then
  echo "错误：dmg 内未找到 .app" >&2
  hdiutil detach "$MNT" -quiet 2>/dev/null || true
  exit 1
fi

echo "==> 安装到 /Applications 并清除隔离属性"
rm -rf "/Applications/$APP_NAME.app"
cp -R "$SRC" "/Applications/$APP_NAME.app"
xattr -cr "/Applications/$APP_NAME.app" 2>/dev/null || true
hdiutil detach "$MNT" -quiet 2>/dev/null || true

echo "==> 完成：$APP_NAME 已安装到 /Applications。"
echo "    直接打开即可；若仍弹「无法验证开发者」，去 系统设置 → 隐私与安全性 点「仍要打开」。"
