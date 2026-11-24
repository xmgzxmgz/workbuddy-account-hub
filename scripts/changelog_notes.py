#!/usr/bin/env python3
"""把 CHANGELOG.md 的某个版本段落，转成 GitHub Release 正文。

## 为什么是"搬运"而不是"压缩"

第一版我试过自动摘要（抽加粗小标题、按关键词分类），结果抽出来的是
"用户截图实录""bump 0.6.31 → 0.6.32"这类过程性噪声——因为这份 CHANGELOG 的写法
是"真因 → 修法 → 验证"的叙事结构，要点藏在内层缩进里，正则会把它切碎。

而这份 CHANGELOG 的详细程度恰恰是最值钱的东西（很多人愿意读这种"我遇到了什么坑、
怎么修的"）。所以这里的策略是：**正文原样搬运，只做机械清理**；
一句话摘要靠约定的 `>` 引用块提供——写 CHANGELOG 的人顺手写一行，Release 就有了好开头。

## CHANGELOG 约定（本脚本依赖它）

```markdown
## v0.6.33（2026-11-20）— 🎯 标题

> 一句话摘要：说清"用户感知到什么变化"，不写实现。没有这行就回退用标题。
```

## 用法

    python3 changelog_notes.py --tag v0.6.32                 # 打印
    python3 changelog_notes.py --tag v0.6.32 --out b.md      # 写入文件
    python3 changelog_notes.py --tag v0.6.32 --github-output # 写进 $GITHUB_OUTPUT（CI）
    python3 changelog_notes.py --list-missing                # 列出版本清单
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

CHANGELOG = Path(__file__).resolve().parents[1] / "CHANGELOG.md"

HEADING_RE = re.compile(r"^##\s+(v\d+\.\d+\.\d+)\s*[（(]([^）)]*)[）)]\s*[—-]?\s*(.*)$")
BUMP_RE = re.compile(r"^\s*[-*]?\s*bump\s+[\d.]+\s*(→|->|⇒)\s*[\d.]+", re.I)
EMOJI_PREFIX = re.compile(r"^[^\w\u4e00-\u9fff（(]+")


def parse_versions(text: str) -> list[dict]:
    versions: list[dict] = []
    current: dict | None = None
    for raw in text.splitlines():
        m = HEADING_RE.match(raw)
        if m:
            if current:
                versions.append(current)
            current = {
                "version": m.group(1),
                "date": m.group(2).strip(),
                "title": m.group(3).strip(),
                "lines": [],
            }
        elif current is not None:
            current["lines"].append(raw)
    if current:
        versions.append(current)
    return versions


def extract_summary_and_body(lines: list[str]) -> tuple[str | None, list[str]]:
    """取第一段 `> 引用块` 作为摘要，其余作为正文。"""
    summary_parts: list[str] = []
    body: list[str] = []
    in_quote = False
    for line in lines:
        if line.startswith(">"):
            in_quote = True
            summary_parts.append(line.lstrip("> ").rstrip())
            continue
        if in_quote:
            in_quote = False
            if not line.strip():
                continue
        body.append(line)
    summary = " ".join(p for p in summary_parts if p).strip() or None
    return summary, body


def clean_body(lines: list[str]) -> str:
    """机械清理：去掉 bump 行、尾部 ---、demote 标题层级、压缩连续空行。"""
    out: list[str] = []
    for line in lines:
        if BUMP_RE.match(line):
            continue
        if re.match(r"^#{3,}\s", line):
            out.append("####" + line[3:] if line.startswith("###") else "###" + line[2:])
        elif re.match(r"^##\s", line):
            out.append("###" + line[2:])
        else:
            out.append(line)
    text = "\n".join(out)
    text = re.sub(r"\n{3,}", "\n\n", text)
    text = re.sub(r"\n+---+\s*$", "", text)
    return text.strip()


def render(v: dict, summary_override: str | None = None) -> str:
    summary, body_lines = extract_summary_and_body(v["lines"])
    summary = summary_override or summary or v["title"]
    body = clean_body(body_lines)
    if not body:
        body = "_本次版本无用户可见改动，仅为内部调整。_"

    out = [f"**{v['title']}**", "", summary, "", "---", "", body, "", "### 升级方式", ""]
    out += [
        "| 平台 | 方式 |",
        "|---|---|",
        "| Windows | 下载 `..._x64-setup.exe` 覆盖安装；免安装用 `..._portable-x64.zip` |",
        "| macOS | 下载 `..._aarch64.dmg` 拖进 Applications |",
        "",
        f"<sub>{v['version']} · {v['date']} · "
        f"[完整更新日志](https://github.com/xmgzxmgz/workbuddy-account-hub/blob/main/CHANGELOG.md)</sub>",
    ]
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag")
    ap.add_argument("--out")
    ap.add_argument("--summary", help="覆盖一句话摘要")
    ap.add_argument("--github-output", action="store_true")
    ap.add_argument("--list-missing", action="store_true")
    ap.add_argument("--changelog", default=str(CHANGELOG))
    args = ap.parse_args()

    versions = parse_versions(Path(args.changelog).read_text(encoding="utf-8"))

    if args.list_missing:
        for v in versions:
            print(v["version"])
        return 0
    if not args.tag:
        ap.error("需要 --tag 或 --list-missing")

    target = next((v for v in versions if v["version"] == args.tag), None)
    if not target:
        print(f"[error] 找不到 {args.tag}；可选 {[v['version'] for v in versions[:5]]}", file=sys.stderr)
        return 1

    body = render(target, args.summary)

    if args.out:
        Path(args.out).write_text(body + "\n", encoding="utf-8")
        print(f"[ok] {args.tag} -> {args.out} ({len(body)} chars)", file=sys.stderr)
    if args.github_output:
        gh_out = os.environ.get("GITHUB_OUTPUT")
        if not gh_out:
            print("[error] 不在 Actions 环境（缺 $GITHUB_OUTPUT）", file=sys.stderr)
            return 1
        with open(gh_out, "a", encoding="utf-8") as fh:
            fh.write("body<<RELEASE_NOTES_EOF\n" + body + "\nRELEASE_NOTES_EOF\n")
    if not args.out and not args.github_output:
        print(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
