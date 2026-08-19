#!/usr/bin/env node
// sync-server.js — 零依赖记录级同步服务器
// 数据模型：每个 account 下 records map（按 id 索引），每条 = {id,type,owner_uid,data,version,updated_at,deleted}
// 合并策略：LWW（last-write-wins），先比 updated_at，平局比 version
// 存储：JSON 文件，单进程写入（PoC 级；生产可换 Postgres + 对象存储）
//
// 启动： node sync-server.js [--port 8787] [--data ./data]
'use strict';
const http = require('http');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const getOpt = (name, def) => { const i = process.argv.indexOf('--' + name); return i >= 0 ? process.argv[i + 1] : def; };
const PORT = Number(getOpt('port', process.env.SYNC_PORT || '8787'));
const DATA_DIR = getOpt('data', path.join(__dirname, 'data'));
fs.mkdirSync(DATA_DIR, { recursive: true });

// ---- 账户（账号注册/鉴权） ----
const ACCOUNTS_FILE = path.join(DATA_DIR, 'accounts.json');
const loadAccounts = () => { try { return JSON.parse(fs.readFileSync(ACCOUNTS_FILE, 'utf8')); } catch { return {}; } };
const saveAccounts = a => fs.writeFileSync(ACCOUNTS_FILE, JSON.stringify(a, null, 2));
let accounts = loadAccounts();
const sha = s => crypto.createHash('sha256').update(String(s)).digest('hex');

// ---- records 读写 ----
const sanitize = s => String(s).replace(/[^a-zA-Z0-9_-]/g, '_');
const recFile = id => path.join(DATA_DIR, sanitize(id), 'records.json');
const loadRecs = id => { try { return JSON.parse(fs.readFileSync(recFile(id), 'utf8')); } catch { return {}; } };
const saveRecs = (id, r) => { fs.mkdirSync(path.join(DATA_DIR, sanitize(id)), { recursive: true }); fs.writeFileSync(recFile(id), JSON.stringify(r, null, 2)); };

// ---- 鉴权：Basic base64(accountId:token) ----
function auth(req) {
  const h = req.headers['authorization'] || '';
  if (!h.startsWith('Basic ')) return null;
  try {
    const [id, tok] = Buffer.from(h.slice(6), 'base64').toString('utf8').split(':');
    const a = accounts[id];
    if (a && a.tokenHash === sha(tok)) return id;
  } catch {}
  return null;
}

// ---- LWW 合并 ----
function merge(store, rec) {
  if (!rec || !rec.id) return 'invalid';
  const cur = store[rec.id];
  if (!cur) { store[rec.id] = rec; return 'added'; }
  if (rec.updated_at > cur.updated_at) { store[rec.id] = rec; return 'updated'; }
  if (rec.updated_at === cur.updated_at && (rec.version || 0) > (cur.version || 0)) { store[rec.id] = rec; return 'updated'; }
  return 'skipped'; // 本地较新，保留
}

function send(res, code, obj, cors = true) {
  const headers = { 'Content-Type': 'application/json' };
  if (cors) {
    headers['Access-Control-Allow-Origin'] = '*';
    headers['Access-Control-Allow-Headers'] = 'Authorization,Content-Type';
    headers['Access-Control-Allow-Methods'] = 'GET,POST,OPTIONS';
  }
  res.writeHead(code, headers);
  res.end(JSON.stringify(obj));
}
function readBody(req) {
  return new Promise((ok, err) => {
    let b = '';
    req.on('data', d => (b += d));
    req.on('end', () => { try { ok(b ? JSON.parse(b) : {}); } catch (e) { err(e); } });
  });
}

const server = http.createServer(async (req, res) => {
  if (req.method === 'OPTIONS') {
    res.writeHead(204, { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Headers': 'Authorization,Content-Type', 'Access-Control-Allow-Methods': 'GET,POST,OPTIONS' });
    return res.end();
  }
  const url = new URL(req.url, 'http://x');
  try {
    // 注册（首次设定 token；若已存在且 token 不符则拒绝重设）
    if (req.method === 'POST' && url.pathname === '/api/register') {
      const { accountId, token } = await readBody(req);
      if (!accountId || !token) return send(res, 400, { error: 'accountId and token required' });
      if (accounts[accountId] && accounts[accountId].tokenHash !== sha(token))
        return send(res, 409, { error: 'accountId exists with a different token' });
      accounts[accountId] = { tokenHash: sha(token), created_at: Date.now() };
      saveAccounts(accounts);
      return send(res, 200, { ok: true, accountId });
    }
    // 以下需鉴权
    const aid = auth(req);
    if (!aid) return send(res, 401, { error: 'unauthorized' });
    if (req.method === 'GET' && url.pathname === '/api/pull') {
      const since = Number(url.searchParams.get('since') || 0);
      const recs = loadRecs(aid);
      const out = Object.values(recs).filter(r => r.updated_at >= since);
      return send(res, 200, { records: out, server_now: Date.now() });
    }
    if (req.method === 'POST' && url.pathname === '/api/push') {
      const { records } = await readBody(req);
      const recs = loadRecs(aid);
      const stats = { added: 0, updated: 0, skipped: 0, invalid: 0 };
      for (const r of (records || [])) { const s = merge(recs, r); stats[s] = (stats[s] || 0) + 1; }
      saveRecs(aid, recs);
      return send(res, 200, { ok: true, ...stats, server_now: Date.now() });
    }
    if (req.method === 'GET' && url.pathname === '/api/status') {
      const recs = loadRecs(aid);
      return send(res, 200, { accountId: aid, record_count: Object.keys(recs).length, server_now: Date.now() });
    }
    return send(res, 404, { error: 'not found' });
  } catch (e) {
    return send(res, 500, { error: String((e && e.message) || e) });
  }
});

server.listen(PORT, '127.0.0.1', () => console.log(`[sync-server] listening on http://127.0.0.1:${PORT}  data=${DATA_DIR}`));
