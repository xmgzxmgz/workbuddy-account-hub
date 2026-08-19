#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WorkBuddy 账户中心 — 只读探测器 (P1 PoC, 阶段 1)
================================================
目的：在不修改任何文件的前提下，探测本机 WorkBuddy 的：
  1. 多账号登录态登记表 (local_storage/entry_*.info 中的 list[{userId,data}])
  2. 各账号在 workbuddy.db 的会话归属 (交叉验证“当前账号”)
  3. Keychain 中的 Safe Storage 加密密钥 (登录凭据加密依赖它)
  4. 切换账号时需要整体备份/恢复的 local_storage 文件清单

这是 P1 账户管理器的“只读核心”，后续 snapshot/restore 在其上扩展。
绝不写 Keychain、绝不改 workbuddy.db、绝不删文件。
"""
import json
import glob
import os
import sqlite3
import subprocess

WB = os.path.expanduser("~/.workbuddy")
LS = os.path.join(WB, "local_storage")
DB = os.path.join(WB, "workbuddy.db")
KEYCHAIN_SVC = "CodeBuddy Safe Storage"


def find_registry():
    """扫描 local_storage，返回 (registry_path, accounts_list)。
    registry 特征：JSON list，每个元素是含 userId/data/ts 的 dict。"""
    best = (None, [])
    for f in sorted(glob.glob(os.path.join(LS, "entry_*.info"))):
        try:
            d = json.load(open(f, encoding="utf-8"))
        except Exception:
            continue
        if isinstance(d, list) and d and all(
            isinstance(x, dict) and "userId" in x for x in d
        ):
            # 取账号数最多的那个作为登记文件
            if len(d) >= len(best[1]):
                best = (f, d)
    return best


def db_user_counts():
    if not os.path.exists(DB):
        return {"_error": "workbuddy.db 不存在"}
    try:
        c = sqlite3.connect(DB)
        cur = c.cursor()
        cur.execute("SELECT user_id, COUNT(*) FROM sessions GROUP BY user_id")
        rows = {r[0]: r[1] for r in cur.fetchall()}
        c.close()
        return rows
    except Exception as e:  # noqa
        return {"_error": str(e)}


def keychain_present():
    try:
        r = subprocess.run(
            ["security", "find-generic-password", "-s", KEYCHAIN_SVC],
            capture_output=True,
            text=True,
            timeout=10,
        )
        return r.returncode == 0
    except Exception:  # noqa
        return False


def main():
    reg_path, accounts = find_registry()
    print("=== WorkBuddy 账户中心 · 只读探测 ===\n")

    print(f"[登记文件] {os.path.basename(reg_path) if reg_path else '未找到'}")
    print(f"[账号数]   {len(accounts)}")
    for a in accounts:
        data = a.get("data")
        nkeys = len(data) if isinstance(data, dict) else "?"
        print(f"  - userId: {a.get('userId')}  data 键数: {nkeys}")

    print("\n[DB 会话归属 — 交叉验证当前账号]")
    counts = db_user_counts()
    if "_error" in counts:
        print(f"  {counts['_error']}")
    else:
        for uid, n in counts.items():
            print(f"  {uid}: {n} 条")

    print(f"\n[Keychain 凭据 '{KEYCHAIN_SVC}'] {'存在' if keychain_present() else '未找到'}")
    print("  (该密钥用于解密 localStorage 中的登录凭据，恢复登录态必须一并备份)")

    ls_files = sorted(os.path.basename(x) for x in glob.glob(os.path.join(LS, "*.info")))
    print(f"\n[local_storage 文件数] {len(ls_files)} (切换账号时需整体备份/恢复)")
    print("  清单:", ", ".join(ls_files[:6]) + (" ..." if len(ls_files) > 6 else ""))


if __name__ == "__main__":
    main()
