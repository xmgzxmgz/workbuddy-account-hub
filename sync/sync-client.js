#!/usr/bin/env node
// sync-client.js — 提取本地 WorkBuddy 数据并 push/pull 到同步服务器
// 需要 Node 22+ 的实验性 node:sqlite： node --experimental-sqlite sync-client.js <cmd> ...
//
// 用法：
//   extract  --uid <uid> [--db <path>] [--wb <~/.workbuddy>]   输出 records JSON 到 stdout（不推送）
//   push     --account A --token T --uid X [--db] [--wb]       提取并推送
//   pull     --account A --token T --uid X [--db] [--wb] [--apply]  拉取；默认 dry-run，--apply 才写回
//   status   --account A --token T                            查看服务器端记录数
//
// 同步语义：服务器按 record.id 聚合所有设备/账号的数据；pull --apply 时把远端记录映射到
//   本设备“当前登录 uid”（owner_uid 仅作来源标记），实现跨账号/跨设备会话与记忆统一。
'use strict';
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const HOME = process.env.HOME;
const WB = () => process.env.WORKBUDDY_HOME || path.join(HOME, '.workbuddy');
const DB = () => path.join(WB(), 'workbuddy.db');

// ---- 命令行参数 ----
function getArgs() {
  const a = { _: [] };
  for (let i = 2; i < process.argv.length; i++) {
    const x = process.argv[i];
    if (x.startsWith('--')) {
      const k = x.slice(2);
      const v = process.argv[i + 1];
      if (v === undefined || v.startsWith('--')) a[k] = true;
      else { a[k] = v; i++; }
    } else a._.push(x);
  }
  return a;
}
const args = getArgs();
const SERVER = args.server || process.env.SYNC_SERVER || 'http://127.0.0.1:8787';
const authHeader = (account, token) => ({
  'Content-Type': 'application/json',
  'Authorization': 'Basic ' + Buffer.from(`${account}:${token}`).toString('base64'),
});
function isAppRunning() {
  try { return execSync('pgrep -f "WorkBuddy"', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim().length > 0; }
  catch { return false; }
}

// ---- 提取本地数据为 records ----
function extract(uid, dbPath, wbPath) {
  const DatabaseSync = require('node:sqlite').DatabaseSync;
  const db = new DatabaseSync(dbPath || DB(), { readOnly: true });
  const records = {};
  // 1) 会话元数据
  let rows = [];
  try { rows = db.prepare('SELECT * FROM sessions WHERE user_id=?').all(uid); } catch (e) { console.error('读 sessions 失败:', e.message); }
  db.close();
  for (const r of rows) {
    // 软删除必须把时间戳 bump 到 deleted_at，删除信号才能靠 LWW(updated_at) 传播
    const ua = r.deleted_at ? Math.max(r.updated_at || 0, r.deleted_at) : (r.updated_at || r.created_at || Date.now());
    records[`session:${r.id}`] = {
      id: `session:${r.id}`, type: 'session', owner_uid: uid, data: r,
      version: 1, updated_at: ua, deleted: !!r.deleted_at,
    };
  }
  // 2) 长期记忆
  const memPath = path.join(wbPath || WB(), 'memory', `${uid}_memory.md`);
  if (fs.existsSync(memPath)) {
    const content = fs.readFileSync(memPath, 'utf8');
    const m = fs.statSync(memPath);
    records[`memory:${uid}`] = { id: `memory:${uid}`, type: 'memory', owner_uid: uid, data: { content }, version: 1, updated_at: m.mtimeMs, deleted: false };
  }
  // 3) 连接器配置
  const connDir = path.join(wbPath || WB(), 'connectors', uid);
  if (fs.existsSync(connDir)) {
    for (const f of fs.readdirSync(connDir)) {
      if (f.endsWith('.json')) {
        try {
          const c = JSON.parse(fs.readFileSync(path.join(connDir, f), 'utf8'));
          const id = `connector:${uid}:${f}`;
          const st = fs.statSync(path.join(connDir, f));
          records[id] = { id, type: 'connector', owner_uid: uid, data: { file: f, content: c }, version: 1, updated_at: st.mtimeMs, deleted: false };
        } catch {}
      }
    }
  }
  return Object.values(records);
}

// ---- push ----
async function push(uid, account, token) {
  const recs = extract(uid, args.db, args.wb);
  const res = await fetch(`${SERVER}/api/push`, { method: 'POST', headers: authHeader(account, token), body: JSON.stringify({ records: recs }) });
  const j = await res.json();
  console.log(`push: 提取 ${recs.length} 条 -> 服务器响应`, j);
  return j;
}

// ---- pull（dry-run 或 apply） ----
async function pull(uid, account, token, apply) {
  const local = {};
  for (const r of extract(uid, args.db, args.wb)) local[r.id] = r;
  const res = await fetch(`${SERVER}/api/pull?since=0`, { headers: authHeader(account, token) });
  const { records: remote } = await res.json();
  const remoteById = {};
  for (const r of remote) remoteById[r.id] = r;

  const plan = [];
  for (const r of remote) {
    const l = local[r.id];
    if (!l) plan.push({ action: r.deleted ? 'add(但远端已删,跳过)' : 'add', id: r.id, type: r.type });
    else if (r.updated_at > l.updated_at || (r.updated_at === l.updated_at && (r.version || 0) > (l.version || 0)))
      plan.push({ action: r.deleted ? 'delete' : 'update', id: r.id, type: r.type });
  }
  if (!apply) {
    console.log(`DRY-RUN：本地 ${Object.keys(local).length} 条，远端 ${remote.length} 条，变更计划 ${plan.length} 项：`);
    for (const p of plan) console.log(`  ${String(p.action).padEnd(14)} ${p.id}`);
    console.log('（未应用；加 --apply 才会写回本地，且需先退出 WorkBuddy）');
    return plan;
  }
  if (isAppRunning() && !args.force) { console.error('✗ WorkBuddy 正在运行，请先 Cmd+Q 退出后再 --apply（若确认目标库非活跃，可加 --force 跳过此检查）'); process.exit(2); }
  applyPlan(plan, remoteById, uid);
  console.log(`✓ 已应用 ${plan.length} 项变更到本地`);
}

function applyPlan(plan, remoteById, uid) {
  const DatabaseSync = require('node:sqlite').DatabaseSync;
  const db = new DatabaseSync(DB());
  for (const p of plan) {
    const r = remoteById[p.id];
    if (p.type === 'session') {
      const d = r.data;
      if (r.deleted) { db.prepare('UPDATE sessions SET deleted_at=? WHERE id=?').run(Date.now(), d.id); continue; }
      const cols = Object.keys(d);
      const exists = db.prepare('SELECT 1 FROM sessions WHERE id=?').get(d.id);
      if (exists) {
        const setCols = cols.map(c => `${c}=?`).join(',');
        const vals = cols.map(c => (c === 'user_id' ? uid : d[c]));
        db.prepare(`UPDATE sessions SET ${setCols} WHERE id=?`).run(...vals, d.id);
      } else {
        const vals = cols.map(c => (c === 'user_id' ? uid : d[c]));
        db.prepare(`INSERT INTO sessions (${cols.join(',')}) VALUES (${cols.map(() => '?').join(',')})`).run(...vals);
      }
    } else if (p.type === 'memory') {
      if (!r.deleted) fs.writeFileSync(path.join(WB(), 'memory', `${uid}_memory.md`), r.data.content || '');
    } else if (p.type === 'connector') {
      if (!r.deleted) {
        const cd = path.join(WB(), 'connectors', uid);
        fs.mkdirSync(cd, { recursive: true });
        fs.writeFileSync(path.join(cd, r.data.file), JSON.stringify(r.data.content, null, 2));
      }
    }
  }
  db.close();
}

// ---- 入口 ----
(async () => {
  const cmd = args._[0];
  if (cmd === 'extract') {
    if (!args.uid) return console.error('extract 需要 --uid');
    const recs = extract(args.uid, args.db, args.wb);
    console.log(JSON.stringify(recs, null, 2));
  } else if (cmd === 'push') {
    if (!args.account || !args.token || !args.uid) return console.error('push 需要 --account --token --uid');
    await push(args.uid, args.account, args.token);
  } else if (cmd === 'pull') {
    if (!args.account || !args.token || !args.uid) return console.error('pull 需要 --account --token --uid');
    await pull(args.uid, args.account, args.token, !!args.apply);
  } else if (cmd === 'status') {
    if (!args.account || !args.token) return console.error('status 需要 --account --token');
    const res = await fetch(`${SERVER}/api/status`, { headers: authHeader(args.account, args.token) });
    console.log(await res.json());
  } else {
    console.log(`用法:
  node --experimental-sqlite sync-client.js extract --uid <uid>
  node --experimental-sqlite sync-client.js push    --account A --token T --uid X
  node --experimental-sqlite sync-client.js pull    --account A --token T --uid X [--apply] [--force]
  node --experimental-sqlite sync-client.js status  --account A --token T`);
  }
})();
