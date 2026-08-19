#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WorkBuddy 账户中心 — 一键切换脚本 (P1 收尾)
==========================================
用途：在多个已快照的账号之间一键切换。
流程：切换前自动快照“当前账号”（确保随时能退回） → 退出 WorkBuddy →
      恢复目标账号快照(--apply) → 重启 WorkBuddy。

重要：本脚本会退出并重启 WorkBuddy，必须由你在终端自行运行，
      不要通过 WorkBuddy 内部执行（会终止宿主自身）。
"""
import argparse
import getpass
import subprocess
import sys
import time
from pathlib import Path

import vault
import detect

APP = "WorkBuddy"
VAULT = vault.DEFAULT_VAULT


def current_uid():
    _, accounts = detect.find_registry()
    return accounts[0].get("userId") if accounts else None


def quit_app(timeout=20):
    print(f"[switch] 优雅退出 {APP} ...")
    subprocess.run(["osascript", "-e", f'quit app "{APP}"'],
                   capture_output=True, text=True)
    for _ in range(timeout * 2):
        if not vault.app_running():
            print("  已退出。")
            return True
        time.sleep(0.5)
    print("  常规退出超时，尝试强制结束...")
    subprocess.run(["pkill", "-f", APP], capture_output=True, text=True)
    time.sleep(2)
    return not vault.app_running()


def relaunch():
    print(f"[switch] 重启 {APP} ...")
    subprocess.run(["open", "-a", APP], capture_output=True, text=True)


def pick_snapshot(uid, name):
    rows = [m for m in vault.list_snapshots(VAULT) if m.get("uid") == uid]
    if name:
        rows = [m for m in rows if m.get("name") == name]
    if not rows:
        return None
    rows.sort(key=lambda m: m.get("ts", ""), reverse=True)
    return rows[0]


def do_switch(uid, name, passphrase):
    cur = current_uid()
    print(f"[switch] 当前账号: {cur}")
    if cur and cur == uid:
        print(f"  目标就是当前账号 {uid}，无需切换。")
        return

    # 1) 切换前快照当前账号（保险）
    if cur:
        vault.snapshot(cur, VAULT, passphrase, name="auto-pre-switch")
        print(f"  已自动快照当前账号 {cur}（可用于退回）。")

    # 2) 选目标快照
    snap_meta = pick_snapshot(uid, name)
    if not snap_meta:
        print(f"  !! 找不到账号 {uid} 的快照。请先对该账号执行 vault.py snapshot。")
        return
    snap_enc = VAULT / uid / f"{snap_meta['ts']}{('_'+name) if name else ''}.tar.gz.enc"

    # 3) 退出应用
    if vault.app_running():
        if not quit_app():
            print("  !! 无法退出 WorkBuddy，中止切换以免损坏文件。")
            return

    # 4) 恢复目标
    ok = vault.restore(snap_enc, passphrase, apply=True)
    if not ok:
        print("  !! 恢复失败，未做改动。")
        return

    # 5) 重启
    relaunch()
    print(f"[switch] 完成：已切换到账号 {uid}。")


def main():
    p = argparse.ArgumentParser(description="WorkBuddy 一键切换账号 (终端自行运行)")
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("list", help="列出可切换的账号快照")

    tp = sub.add_parser("to", help="切换到指定账号")
    tp.add_argument("--uid", required=True)
    tp.add_argument("--name", default=None, help="若同一账号有多份快照，指定标签")
    tp.add_argument("--passphrase", default=None)

    args = p.parse_args()

    if args.cmd == "list":
        rows = vault.list_snapshots(VAULT)
        cur = current_uid()
        if not rows:
            print("（无快照。请先对每个账号运行 vault.py snapshot --uid <uid>）")
        for m in rows:
            mark = "  <- 当前" if m.get("uid") == cur else ""
            print(f"  uid={m.get('uid')}  ts={m.get('ts')}  name={m.get('name') or '-'}  "
                  f"文件数={m.get('file_count')}{mark}")
        return

    if args.cmd == "to":
        ph = args.passphrase or (getpass.getpass("保险库 passphrase: ") if vault.HAVE_CRYPTO else "")
        do_switch(args.uid, args.name, ph)


if __name__ == "__main__":
    main()
