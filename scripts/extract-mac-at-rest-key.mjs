#!/usr/bin/env node
/**
 * WorkBuddy macOS at-rest 密钥一次性提取脚本。
 *
 * 背景：WorkBuddy macOS 客户端把登录态文件（workbuddy-desktop.info）中的
 * accessToken / refreshToken 等字段用 AES-256-GCM 加密，密钥
 * （bootstrap.atRestSecretKey）只存在于 WorkBuddy 客户端进程内部
 * （原生模块 workbuddyStorage.loggerGet()）。本脚本通过 Node Inspector
 * 在 WorkBuddy 主进程上下文里调用该原生 API，取出密钥并保存到
 *   ~/.workbuddy-account-hub/at-rest-key
 * 供 WorkBuddy Account Hub（Tauri/Rust）解密登录态使用。
 *
 * 原理：
 *   1. 以 `--inspect-brk` 启动第二个 WorkBuddy 实例（暂停在裸 V8 上下文，
 *      需发送 Runtime.runIfWaitingForDebugger 才会继续）；
 *   2. 放行后高频轮询 `typeof process`，在「Node 环境就绪、单实例锁退出之前」
 *      的窗口期内抢到主进程上下文；
 *   3. 调用 process._linkedBinding('electron_browser_workbuddy_storage').loggerGet()
 *      拿到 bootstrap，取 atRestSecretKey；
 *   4. 校验 SHA-256(SHA-256(secret)) 前 16 hex == ~/.workbuddy/keyblob 的
 *      slot.protectorKeyId（确认密钥与本机匹配）；
 *   5. 写入密钥文件（0600 权限）。
 *
 * 用法：node scripts/extract-mac-at-rest-key.mjs   （需 Node ≥ 21，内置 WebSocket/fetch）
 * 说明：全程只会短暂启动一个被断点暂停的 WorkBuddy 第二实例，不会影响正在运行的客户端。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawn } from "node:child_process";

const APP = "/Applications/WorkBuddy.app/Contents/MacOS/Electron";
const KEY_OUT = path.join(os.homedir(), ".workbuddy-account-hub", "at-rest-key");
const KEYBLOB = path.join(os.homedir(), ".workbuddy", "keyblob");
const PORT = 9231;

if (process.platform !== "darwin") {
  console.error("本脚本仅用于 macOS（Windows 版登录态是明文，无需提取）。");
  process.exit(1);
}
if (!fs.existsSync(APP)) {
  console.error(`未找到 WorkBuddy: ${APP}`);
  process.exit(1);
}

function fail(msg) {
  console.error("✗ " + msg);
  process.exit(1);
}

// ---------- 1. 启动带 inspector 的第二实例 ----------
const child = spawn(APP, [`--inspect-brk=${PORT}`], {
  stdio: "ignore",
  env: { ...process.env, ELECTRON_RUN_AS_NODE: "" },
  detached: false,
});

const cleanExit = (code) => {
  try { child.kill("SIGKILL"); } catch {}
  process.exit(code);
};
process.on("SIGINT", () => cleanExit(130));

// 等 inspector 端口就绪
async function waitInspector() {
  for (let i = 0; i < 60; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/json/list`);
      return (await r.json()).find((x) => x.webSocketDebuggerUrl);
    } catch { await new Promise((r) => setTimeout(r, 250)); }
  }
  fail("inspector 端口未就绪（15s 超时）");
}

try {
  const target = await waitInspector();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.addEventListener("open", res, { once: true }); ws.addEventListener("error", rej, { once: true }); });

  let seq = 0;
  const pending = new Map();
  ws.addEventListener("message", (ev) => {
    try {
      const m = JSON.parse(ev.data);
      if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
    } catch {}
  });
  const send = (method, params) => new Promise((res) => {
    const id = ++seq;
    pending.set(id, res);
    ws.send(JSON.stringify({ id, method, params }));
  });
  const evalE = async (expr, ms = 8000) => {
    const r = await Promise.race([
      send("Runtime.evaluate", { expression: expr, returnByValue: true }),
      new Promise((_, rej) => setTimeout(() => rej(new Error("evaluate 超时")), ms)),
    ]);
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || "evaluate 异常");
    return r.result?.result?.value;
  };

  // ---------- 2. 放行 + 抢窗口期 ----------
  await send("Debugger.enable", {});
  await new Promise((r) => setTimeout(r, 300));
  await send("Runtime.runIfWaitingForDebugger", {});

  let ready = false;
  for (let i = 0; i < 1500; i++) {
    try {
      if ((await evalE("typeof process", 1000)) === "object") { ready = true; break; }
    } catch {}
  }
  if (!ready) fail("主进程上下文始终未就绪（process 不可用），提取中止");

  // ---------- 3. 调原生 loggerGet ----------
  const raw = await evalE(`(() => {
    const b = process._linkedBinding('electron_browser_workbuddy_storage');
    const v = b.loggerGet();
    if (v && typeof v.then === 'function') return v.then(x => JSON.stringify(x));
    return JSON.stringify(v);
  })()`);
  const bootstrap = JSON.parse(JSON.parse(raw));

  const secret = bootstrap.atRestSecretKey;
  if (!secret) fail("bootstrap 中没有 atRestSecretKey（客户端版本可能变化，需重新逆向）");

  // ---------- 4. 与本机 keyblob 校验 ----------
  const kb = JSON.parse(fs.readFileSync(KEYBLOB, "utf8"));
  const want = kb?.slots?.[0]?.protectorKeyId;
  const got = crypto.createHash("sha256").update(crypto.createHash("sha256").update(secret, "utf8").digest()).digest("hex").slice(0, 16);
  if (want && got !== want) fail(`密钥与本机不匹配（算得 ${got}，keyblob 期望 ${want}）`);
  console.log(`✓ 密钥校验通过（protectorKeyId=${got}）`);

  // ---------- 5. 落盘（0600） ----------
  fs.mkdirSync(path.dirname(KEY_OUT), { recursive: true });
  fs.writeFileSync(KEY_OUT, secret + "\n", { mode: 0o600 });
  console.log(`✓ 已写入 ${KEY_OUT}`);
  console.log("WorkBuddy Account Hub 现在可以读取 macOS 加密登录态了。");
  ws.close();
  cleanExit(0);
} catch (e) {
  fail(e?.message || String(e));
}
