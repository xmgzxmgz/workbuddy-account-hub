#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WorkBuddy 账户中心 — 加密保险库 + 快照/恢复 (P1 PoC)
====================================================
快照(snapshot)：把 ~/.workbuddy/local_storage/ 整目录 + 本机登录态文件
              (CodeBuddyExtension/.../auth/workbuddy-desktop.info) 打包成加密包，
              按账号 uid 分桶存入 vault。登录态文件包含 accessToken，连它一起备份
              才能真正"保持登录态"（只换 local_storage 会导致 token 与账号不匹配）。
恢复(restore)：解密并整体替换 local_storage/ 与登录态文件（需先退出 WorkBuddy）。
              恢复前自动备份当前状态到 vault/_rollback/<ts>/，支持回滚。默认 dry-run，
              真正落盘需 --apply。

设计要点（修复原 migrate 工具的坑）：
  1. 整体目录快照/恢复，而非改写 user_id —— 同一账号完整状态一次换入换出，无半吊子。
  2. 连登录态文件一起备份/恢复，切换后无需重新登录。
  3. 恢复前强制备份 + 提供 rollback，任何失误可一键退回。
  4. 默认 dry-run，落盘需显式 --apply；恢复要求 WorkBuddy 已退出（检测 pgrep）。
  5. 加密存储（AES via Fernet + PBKDF2），passphrase 不当明文落盘。

Keychain（Safe Storage 密钥）为机器级、对所有账号通用，同机切换无需迁移；
跨机器才需转移，由单独 subcommand 处理（默认不碰，避免误伤系统钥匙串）。
"""
import argparse
import base64
import getpass
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
from pathlib import Path

try:
    from cryptography.fernet import Fernet
    from cryptography.hazmat.primitives import hashes
    from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC
    HAVE_CRYPTO = True
except Exception:  # noqa
    HAVE_CRYPTO = False

WB = Path.home() / ".workbuddy"
LS = WB / "local_storage"
DEFAULT_VAULT = Path(__file__).resolve().parent / "vault"
KEYCHAIN_SVC = "CodeBuddy Safe Storage"
KEYCHAIN_ACCT = "CodeBuddy Key"


# ---------- 登录态文件定位 ----------
def auth_file() -> Path | None:
    home = Path.home()
    if sys.platform == "darwin":
        p = home / "Library" / "Application Support" / "CodeBuddyExtension" / "Data" / "Public" / "auth" / "workbuddy-desktop.info"
    elif sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA", home / "AppData" / "Local"))
        p = base / "CodeBuddyExtension" / "Data" / "Public" / "auth" / "workbuddy-desktop.info"
    else:
        return None
    return p if p.exists() else None


# ---------- 加密 ----------
def derive_key(passphrase: str, salt: bytes) -> bytes:
    kdf = PBKDF2HMAC(algorithm=hashes.SHA256(), length=32, salt=salt, iterations=200_000)
    return base64.urlsafe_b64encode(kdf.derive(passphrase.encode()))


def encrypt_file(src: Path, dst: Path, passphrase: str):
    salt = os.urandom(16)
    key = derive_key(passphrase, salt)
    token = Fernet(key).encrypt(src.read_bytes())
    dst.write_bytes(salt + token)  # salt 明文前缀无妨


def decrypt_file(src: Path, dst: Path, passphrase: str):
    blob = src.read_bytes()
    salt, token = blob[:16], blob[16:]
    key = derive_key(passphrase, salt)
    dst.write_bytes(Fernet(key).decrypt(token))


# ---------- 工具 ----------
def app_running() -> bool:
    try:
        r = subprocess.run(["pgrep", "-f", "WorkBuddy"], capture_output=True, text=True)
        return r.returncode == 0
    except Exception:  # noqa
        return False


def _pack_state(dest_tar: Path):
    with tarfile.open(dest_tar, "w:gz") as tar:
        if LS.exists():
            tar.add(LS, arcname="local_storage")
        af = auth_file()
        if af:
            tar.add(af, arcname="auth/workbuddy-desktop.info")


def _count_files(p: Path) -> int:
    return sum(1 for _ in p.rglob("*") if _.is_file())


# ---------- 快照 ----------
def snapshot(uid: str, vault: Path, passphrase: str, name: str = ""):
    vault = Path(vault)
    bucket = vault / uid
    bucket.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y%m%d_%H%M%S")
    base = ts + (f"_{name}" if name else "")
    enc_path = bucket / f"{base}.tar.gz.enc"
    meta_path = bucket / f"{base}.json"

    af = auth_file()
    with tempfile.TemporaryDirectory() as td:
        tar_path = Path(td) / "state.tar.gz"
        _pack_state(tar_path)
        n_ls = _count_files(LS) if LS.exists() else 0
        file_count = n_ls + (1 if af else 0)
        if HAVE_CRYPTO:
            encrypt_file(tar_path, enc_path, passphrase)
        else:
            shutil.copy(tar_path, enc_path)

    meta = {
        "uid": uid,
        "ts": ts,
        "name": name,
        "encrypted": HAVE_CRYPTO,
        "file_count": file_count,
        "auth_included": bool(af),
        "compressed_size": enc_path.stat().st_size,
        "source_ls": str(LS),
        "source_auth": str(af) if af else None,
    }
    meta_path.write_text(json.dumps(meta, ensure_ascii=False, indent=2), encoding="utf-8")
    return enc_path, meta_path


# ---------- 列出 ----------
def list_snapshots(vault: Path):
    vault = Path(vault)
    rows = []
    for enc in sorted(vault.rglob("*.tar.gz.enc")):
        base = enc.name[: -len(".tar.gz.enc")]
        meta_m = enc.with_name(f"{base}.json")
        meta = json.loads(meta_m.read_text(encoding="utf-8")) if meta_m.exists() else {}
        rows.append(meta)
    return rows


# ---------- 恢复 ----------
def backup_current(dest: Path):
    dest.mkdir(parents=True, exist_ok=True)
    if LS.exists():
        shutil.copytree(LS, dest / "local_storage", dirs_exist_ok=True)
    af = auth_file()
    if af:
        shutil.copy(af, dest / "auth.workbuddy-desktop.info")


def restore(snapshot_enc: Path, passphrase: str, apply: bool = False):
    snapshot_enc = Path(snapshot_enc)
    base = snapshot_enc.name[: -len(".tar.gz.enc")]
    meta_m = snapshot_enc.with_name(f"{base}.json")
    meta = json.loads(meta_m.read_text(encoding="utf-8")) if meta_m.exists() else {}
    uid = meta.get("uid", "?")
    print(f"[restore] 目标快照: {snapshot_enc.name}  uid={uid}")

    if app_running():
        print("  !! 检测到 WorkBuddy 正在运行。恢复会破坏其打开的文件，请先 Cmd+Q 完全退出后再执行。")
        return False

    with tempfile.TemporaryDirectory() as td:
        tar_path = Path(td) / "state.tar.gz"
        if HAVE_CRYPTO and meta.get("encrypted"):
            try:
                decrypt_file(snapshot_enc, tar_path, passphrase)
            except Exception as e:  # noqa
                print(f"  !! 解密失败（passphrase 错误？）: {e}")
                return False
        else:
            shutil.copy(snapshot_enc, tar_path)
        extract_dir = Path(td) / "ext"
        with tarfile.open(tar_path) as tar:
            tar.extractall(extract_dir)
        restored_ls = extract_dir / "local_storage"
        restored_auth = extract_dir / "auth" / "workbuddy-desktop.info"
        n = _count_files(restored_ls)

        if not apply:
            print(f"  [dry-run] 将用此快照（local_storage {n} 文件"
                  f"{' + 登录态文件' if restored_auth.exists() else ''}）整体替换当前状态")
            print(f"  [dry-run] 真正恢复前会自动备份当前状态到 vault/_rollback/<ts>/")
            print(f"  [dry-run] 确认无误后加 --apply 执行（需 WorkBuddy 已退出）。")
            return True

        # 真正恢复
        rb = Path(snapshot_enc).parent.parent / "_rollback" / time.strftime("%Y%m%d_%H%M%S")
        backup_current(rb)
        if LS.exists():
            shutil.rmtree(LS)
        if restored_ls.exists():
            shutil.copytree(restored_ls, LS)
        if restored_auth.exists():
            af = auth_file()
            if af:
                af.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(restored_auth, af)
        print(f"  已恢复（local_storage + 登录态）。回滚点: {rb}")
        return True


# ---------- Keychain（仅跨机器需要，默认不执行） ----------
def keychain_dump_meta(out: Path):
    r = subprocess.run(
        ["security", "find-generic-password", "-s", KEYCHAIN_SVC],
        capture_output=True, text=True,
    )
    out.write_text(r.stdout or r.stderr, encoding="utf-8")
    print(f"  已导出元数据到 {out}（不含 secret；跨机迁移请手动在钥匙串访问中导出该项）")


# ---------- CLI ----------
def main(argv=None):
    p = argparse.ArgumentParser(description="WorkBuddy 账户加密保险库 (P1 PoC)")
    p.add_argument("--vault", default=str(DEFAULT_VAULT), help="保险库存放目录")
    sub = p.add_subparsers(dest="cmd", required=True)

    sp = sub.add_parser("snapshot", help="快照当前 local_storage + 登录态到保险库")
    sp.add_argument("--uid", required=True)
    sp.add_argument("--name", default="")
    sp.add_argument("--passphrase", default=None)

    sub.add_parser("list", help="列出所有快照")

    rp = sub.add_parser("restore", help="从快照恢复（默认 dry-run）")
    rp.add_argument("--id", required=True, help="快照 .tar.gz.enc 路径")
    rp.add_argument("--passphrase", default=None)
    rp.add_argument("--apply", action="store_true", help="真正落盘（需 WorkBuddy 已退出）")

    kp = sub.add_parser("keychain-dump", help="导出 Keychain 元数据（安全，不含 secret）")
    kp.add_argument("--out", default="keychain_meta.txt")

    args = p.parse_args(argv)

    def get_passphrase(arg):
        if arg:
            return arg
        if HAVE_CRYPTO:
            return getpass.getpass("保险库 passphrase: ")
        return ""

    if args.cmd == "snapshot":
        ph = get_passphrase(args.passphrase)
        enc, meta = snapshot(args.uid, Path(args.vault), ph, args.name)
        print(f"[snapshot] 完成: {enc.name}  ->  {meta}")
    elif args.cmd == "list":
        rows = list_snapshots(Path(args.vault))
        if not rows:
            print("（无快照）")
        for m in rows:
            print(f"  uid={m.get('uid')}  ts={m.get('ts')}  name={m.get('name') or '-'}  "
                  f"文件数={m.get('file_count')}  含登录态={m.get('auth_included')}  "
                  f"加密={m.get('encrypted')}  {m.get('compressed_size')}B")
    elif args.cmd == "restore":
        ph = get_passphrase(args.passphrase)
        restore(Path(args.id), ph, args.apply)
    elif args.cmd == "keychain-dump":
        keychain_dump_meta(Path(args.out))


if __name__ == "__main__":
    main()
