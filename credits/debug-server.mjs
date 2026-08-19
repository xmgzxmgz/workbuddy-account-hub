#!/usr/bin/env node
// WorkBuddy 积分/账户接口调试器
// 自动读本机登录态，供浏览器快速 probe 各候选接口
// 用法: node debug-server.mjs   然后打开 http://localhost:8765

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import url from 'node:url';
import { execFile } from 'node:child_process';

const PORT = 8765;
const API_BASE = 'https://copilot.tencent.com';

function authCandidates() {
  const home = os.homedir();
  const cands = [];
  if (process.platform === 'darwin') {
    cands.push(path.join(home, 'Library', 'Application Support', 'CodeBuddyExtension', 'Data', 'Public', 'auth', 'workbuddy-desktop.info'));
    cands.push(path.join(home, 'Library', 'Application Support', 'CodeBuddyExtension', 'Data', 'Public', 'auth', 'Tencent-Cloud.coding-copilot.info'));
  } else if (process.platform === 'win32') {
    const local = process.env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
    cands.push(path.join(local, 'CodeBuddyExtension', 'Data', 'Public', 'auth', 'workbuddy-desktop.info'));
  }
  return cands.filter(p => { try { return fs.existsSync(p); } catch { return false; } });
}

function jwtPayload(token) {
  try {
    const p = token.split('.')[1];
    const pad = (4 - (p.length % 4)) % 4;
    const b = Buffer.from(p + '='.repeat(pad), 'base64');
    return JSON.parse(b.toString('utf8'));
  } catch { return null; }
}

function loadLogin() {
  for (const p of authCandidates()) {
    try {
      const d = JSON.parse(fs.readFileSync(p, 'utf8'));
      const token = (d.auth && d.auth.accessToken) || '';
      if (!token) continue;
      const uid = (d.account && d.account.uid) || (jwtPayload(token) || {}).sub || '';
      if (!uid) continue;
      return { token, uid, file: p, account: d.account || {}, raw: d };
    } catch { /* next */ }
  }
  return null;
}

// 返回整个登录态文件（含 accounts / allAccounts 已登记账号列表）
function loadAuthFile() {
  for (const p of authCandidates()) {
    try {
      const d = JSON.parse(fs.readFileSync(p, 'utf8'));
      if (d.auth && d.auth.accessToken) return d;
    } catch { /* next */ }
  }
  return null;
}

// 判断某本地 Data 子目录是否含独立登录态
function dirHasLogin(name) {
  const base = path.join(os.homedir(), 'Library', 'Application Support', 'CodeBuddyExtension', 'Data', name, 'auth');
  try {
    return fs.existsSync(path.join(base, 'workbuddy-desktop.info')) ||
           fs.existsSync(path.join(base, 'Tencent-Cloud.coding-copilot.info'));
  } catch { return false; }
}

function mask(s, n = 8) { return s ? s.slice(0, n) + '…(' + s.length + '字符)' : '(空)'; }

function safeParse(s) {
  try { return JSON.parse(s); } catch { return s; }
}

// 本地 WorkBuddy 数据目录（含各账号 uid 子目录）
function localDataDirs() {
  const home = os.homedir();
  if (process.platform !== 'darwin') return [];
  const base = path.join(home, 'Library', 'Application Support', 'CodeBuddyExtension', 'Data');
  try {
    const entries = fs.readdirSync(base, { withFileTypes: true })
      .filter(e => e.isDirectory())
      .map(e => e.name);
    return entries.map(name => {
      let sizeBytes = 0;
      const dir = path.join(base, name);
      try {
        const walk = p => {
          for (const f of fs.readdirSync(p, { withFileTypes: true })) {
            const fp = path.join(p, f.name);
            try {
              if (f.isDirectory()) walk(fp);
              else sizeBytes += fs.statSync(fp).size;
            } catch { /* ignore */ }
          }
        };
        walk(dir);
      } catch { /* ignore */ }
      return { name, sizeBytes, path: dir };
    });
  } catch { return []; }
}

function duHuman(bytes) {
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = bytes, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return v.toFixed(1) + ' ' + u[i];
}

// App 版本与安装大小
function appEnv() {
  const out = { platform: process.platform, arch: os.arch(), node: process.version, appBundle: '/Applications/WorkBuddy.app' };
  try {
    const plist = path.join(out.appBundle, 'Contents', 'Info.plist');
    if (fs.existsSync(plist)) {
      const s = fs.readFileSync(plist, 'utf8');
      const grab = k => {
        const m = s.match(new RegExp('<key>' + k + '</key>\\s*<string>([^<]*)</string>'));
        return m ? m[1] : null;
      };
      out.version = grab('CFBundleShortVersionString');
      out.build = grab('CFBundleVersion');
    }
  } catch { /* ignore */ }
  try { out.appSize = duHuman(dirSize(out.appBundle)); } catch { out.appSize = null; }
  return out;
}
function dirSize(p) {
  let total = 0;
  const walk = d => {
    for (const f of fs.readdirSync(d, { withFileTypes: true })) {
      const fp = path.join(d, f.name);
      try {
        if (f.isDirectory()) walk(fp);
        else total += fs.statSync(fp).size;
      } catch { /* ignore */ }
    }
  };
  walk(p);
  return total;
}

async function proxyApi(endpoint, method = 'POST', body = '{}', extraHeaders = {}) {
  const login = loadLogin();
  if (!login) throw new Error('未找到本机 WorkBuddy 登录态');
  const isFullUrl = endpoint.startsWith('http://') || endpoint.startsWith('https://');
  const target = isFullUrl ? endpoint : (API_BASE + (endpoint.startsWith('/') ? endpoint : '/' + endpoint));
  const headers = {
    'Content-Type': 'application/json',
    'Accept': 'application/json',
    'Authorization': 'Bearer ' + login.token,
    'X-User-Id': login.uid,
    ...extraHeaders,
  };
  const resp = await fetch(target, {
    method,
    headers,
    body: method === 'GET' || method === 'HEAD' ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)),
  });
  const text = await resp.text();
  return { status: resp.status, headers: Object.fromEntries(resp.headers.entries()), body: text, login: { uid: login.uid, file: login.file, token: mask(login.token) } };
}

let indexHtml = fs.readFileSync(new URL('./web.html', import.meta.url), 'utf8');
const debugHtml = fs.readFileSync(new URL('./debug.html', import.meta.url), 'utf8');

// 本地密钥注入（local-keys.json 仅本机存在、不入库；注入 window.__LOCAL_KEYS__ 供前端预置 API）
try {
  const lkPath = new URL('./local-keys.json', import.meta.url);
  if (fs.existsSync(lkPath)) {
    const lk = JSON.parse(fs.readFileSync(lkPath, 'utf8'));
    indexHtml = indexHtml.replace('window.__LOCAL_KEYS__ = null', 'window.__LOCAL_KEYS__ = ' + JSON.stringify(lk));
  }
} catch { /* ignore */ }

// NAS scp 同步（轻量：账号清单 + API 配置，不含密钥）
function nasScp(action, { host, port, user, pass, config }) {
  return new Promise((resolve) => {
    const tmp = path.join(os.tmpdir(), 'wb-hub-config.json');
    const target = `${user}@${host}:/root/workbuddy-hub/hub-config.json`;
    let args;
    if (action === 'push') {
      try { fs.writeFileSync(tmp, JSON.stringify(config, null, 2)); } catch (e) { resolve({ error: '写临时文件失败: ' + e.message }); return; }
      args = ['-p', pass, 'scp', '-P', String(port), '-o', 'StrictHostKeyChecking=no', tmp, target];
    } else {
      args = ['-p', pass, 'scp', '-P', String(port), '-o', 'StrictHostKeyChecking=no', target, tmp];
    }
    execFile('sshpass', args, { timeout: 25000 }, (err) => {
      if (err) {
        const msg = String(err.message || err);
        resolve({ error: msg.includes('ENOENT') ? '未安装 sshpass，请 brew install sshpass' : 'NAS 连接失败（确认 Tailscale 已连接）: ' + msg.split('\n')[0].slice(0, 120) });
        return;
      }
      if (action === 'push') resolve({ ok: true });
      else {
        try { resolve({ ok: true, config: JSON.parse(fs.readFileSync(tmp, 'utf8')) }); }
        catch { resolve({ error: '拉取文件解析失败（NAS 上可能还没有该配置）' }); }
      }
    });
  });
}

const server = http.createServer(async (req, res) => {
  const parsed = url.parse(req.url, true);
  const pathname = parsed.pathname;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');

  if (pathname === '/' || pathname === '/index.html') {
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(indexHtml);
    return;
  }

  if (pathname === '/debug' || pathname === '/debug.html') {
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(debugHtml);
    return;
  }

  if (pathname === '/token') {
    const login = loadLogin();
    if (!login) { res.statusCode = 404; res.end(JSON.stringify({ error: '未找到本机 WorkBuddy 登录态' }, null, 2)); return; }
    const a = login.account || {};
    res.end(JSON.stringify({
      uid: login.uid,
      nickname: a.nickname,
      type: a.type,
      uin: a.uin,
      phoneNumber: a.phoneNumber,
      lastLogin: a.lastLogin,
      isAdmin: a.isAdmin,
      file: login.file,
      token: mask(login.token),
    }, null, 2));
    return;
  }

  // 解析登录态 JWT 声明 + 有效期
  if (pathname === '/jwt') {
    const login = loadLogin();
    if (!login) { res.statusCode = 404; res.end(JSON.stringify({ error: '未找到登录态' })); return; }
    const pl = jwtPayload(login.token);
    if (!pl) { res.statusCode = 500; res.end(JSON.stringify({ error: 'JWT 解析失败' })); return; }
    const exp = pl.exp, iat = pl.iat, authTime = pl.auth_time;
    const days = exp ? Math.max(0, Math.ceil((exp * 1000 - Date.now()) / 86400000)) : null;
    res.end(JSON.stringify({
      issuer: pl.iss,
      subject: pl.sub,
      audience: pl.aud,
      azp: pl.azp,
      scope: pl.scope,
      email_verified: pl.email_verified,
      preferred_username: pl.preferred_username,
      issued_at: iat ? new Date(iat * 1000).toISOString() : null,
      auth_at: authTime ? new Date(authTime * 1000).toISOString() : null,
      expires_at: exp ? new Date(exp * 1000).toISOString() : null,
      remaining_days: days,
      token_chars: login.token.length,
      token_type: pl.typ,
    }, null, 2));
    return;
  }

  // 本地账号数据目录（各 uid 子目录 + 占用）
  if (pathname === '/local-accounts') {
    const dirs = localDataDirs();
    const login = loadLogin();
    const cur = login ? login.uid : '';
    res.end(JSON.stringify({
      dataRoot: path.join(os.homedir(), 'Library', 'Application Support', 'CodeBuddyExtension', 'Data'),
      current_uid: cur,
      accounts: dirs.map(d => ({ name: d.name, size: duHuman(d.sizeBytes), sizeBytes: d.sizeBytes, is_current: d.name === cur })),
    }, null, 2));
    return;
  }

  // 本机 App 环境信息
  if (pathname === '/env') {
    res.end(JSON.stringify(appEnv(), null, 2));
    return;
  }

  // AI 记忆画像（/api/memory/profile）
  if (pathname === '/memory') {
    try {
      const r = await proxyApi('/api/memory/profile', 'GET', '');
      res.end(JSON.stringify({ status: r.status, body: safeParse(r.body) }, null, 2));
    } catch (e) { res.statusCode = 500; res.end(JSON.stringify({ error: String(e.message || e) })); }
    return;
  }

  // 签到状态（只读）
  if (pathname === '/checkin') {
    try {
      const r = await proxyApi('/billing/meter/checkin-activity-status', 'POST', '{}');
      res.end(JSON.stringify({ status: r.status, body: safeParse(r.body) }, null, 2));
    } catch (e) { res.statusCode = 500; res.end(JSON.stringify({ error: String(e.message || e) })); }
    return;
  }

  // 一键拉全部（前端仪表盘用）
  if (pathname === '/all') {
    const login = loadLogin();
    const authFile = loadAuthFile();
    const cur = login ? login.uid : '';
    if (login) {
      const a = login.account || {};
      var accountProfile = {
        uid: login.uid, nickname: a.nickname, type: a.type, uin: a.uin,
        phoneNumber: a.phoneNumber, lastLogin: a.lastLogin, isAdmin: a.isAdmin,
        isCreator: a.isCreator,
      };
    }
    const result = {
      ok: !!login,
      current_uid: cur,
      login: login ? accountProfile : null,
      registered_accounts: (authFile && authFile.allAccounts) ? authFile.allAccounts.map(x => ({
        uid: x.uid, nickname: x.nickname, type: x.type, phoneNumber: x.phoneNumber,
        lastLogin: x.lastLogin, isCreator: x.isCreator, isAdmin: x.isAdmin,
      })) : [],
    };
    if (login) {
      try {
        const [quota, checkin, memory] = await Promise.all([
          proxyApi('/billing/meter/get-user-resource', 'POST', '{}').catch(e => ({ error: String(e) })),
          proxyApi('/billing/meter/checkin-activity-status', 'POST', '{}').catch(e => ({ error: String(e) })),
          proxyApi('/api/memory/profile', 'GET', '').catch(e => ({ error: String(e) })),
        ]);
        result.quota = quota;
        result.checkin = checkin;
        result.memory = memory;
        const pl = jwtPayload(login.token);
        const exp = pl?.exp;
        result.jwt = {
          issuer: pl?.iss, subject: pl?.sub, audience: pl?.aud, azp: pl?.azp,
          scope: pl?.scope, email_verified: pl?.email_verified, preferred_username: pl?.preferred_username,
          issued_at: pl?.iat ? new Date(pl.iat * 1000).toISOString() : null,
          auth_at: pl?.auth_time ? new Date(pl.auth_time * 1000).toISOString() : null,
          expires_at: exp ? new Date(exp * 1000).toISOString() : null,
          remaining_days: exp ? Math.max(0, Math.ceil((exp * 1000 - Date.now()) / 86400000)) : null,
          token_chars: login.token.length, token_type: pl?.typ,
        };
      } catch (e) { result.error = String(e.message || e); }
    }
    const dirs = localDataDirs();
    result.local_accounts = {
      dataRoot: path.join(os.homedir(), 'Library', 'Application Support', 'CodeBuddyExtension', 'Data'),
      accounts: dirs.map(d => ({
        name: d.name, size: duHuman(d.sizeBytes), sizeBytes: d.sizeBytes,
        has_login: dirHasLogin(d.name),
        is_current: d.name === cur || (d.name === 'Public' && !!cur),
      })),
    };
    result.env = appEnv();
    res.end(JSON.stringify(result, null, 2));
    return;
  }

  if (pathname === '/api' && req.method === 'POST') {
    let raw = '';
    req.on('data', c => raw += c);
    req.on('end', async () => {
      try {
        const j = JSON.parse(raw || '{}');
        const r = await proxyApi(j.endpoint, j.method || 'POST', j.body, j.headers || {});
        res.end(JSON.stringify(r, null, 2));
      } catch (e) {
        res.statusCode = 500;
        res.end(JSON.stringify({ error: String(e.message || e) }, null, 2));
      }
    });
    return;
  }

  // NAS 配置同步（push/pull）
  if (pathname === '/nas-sync' && req.method === 'POST') {
    let raw = '';
    req.on('data', c => raw += c);
    req.on('end', async () => {
      try {
        const j = JSON.parse(raw || '{}');
        if (!j.action || !j.host || !j.port || !j.user || !j.pass) { res.end(JSON.stringify({ error: '缺少 NAS 参数' })); return; }
        const r = await nasScp(j.action, j);
        res.end(JSON.stringify(r));
      } catch (e) {
        res.statusCode = 500;
        res.end(JSON.stringify({ error: String(e.message || e) }));
      }
    });
    return;
  }

  res.statusCode = 404;
  res.end(JSON.stringify({ error: 'not found' }));
});

server.listen(PORT, () => {
  console.log('调试器已启动: http://localhost:' + PORT);
  const login = loadLogin();
  if (login) console.log('已读取登录态:', login.uid, mask(login.token));
  else console.log('未找到登录态，请先登录 WorkBuddy 桌面端');
});
