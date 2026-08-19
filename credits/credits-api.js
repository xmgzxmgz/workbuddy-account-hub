#!/usr/bin/env node
'use strict';
// WorkBuddy 积分 / 签到 —— 读取本机登录态直接调官方接口（无需抓包）
// 参考: github.com/codeLong1024/workbuddy-checkin
// 接口: https://copilot.tencent.com/v2/billing/meter/{checkin-activity-status,daily-checkin}
//
// 用法:
//   node credits-api.js status    # 查询签到活动状态（只读，安全）
//   node credits-api.js quota     # 查询账号剩余积分/资源包/到期时间
//   node credits-api.js checkin   # 执行今日签到（幂等，已签则跳过）
//   node credits-api.js token     # 显示本机登录态信息（脱敏，调试用）
//
// 安全: 仅读取本机 WorkBuddy 登录态文件，不存储任何凭证；不依赖 mitmproxy。

const fs = require('fs');
const os = require('os');
const path = require('path');

const API_BASE_CHECKIN = 'https://copilot.tencent.com/v2/billing/meter';
const API_BASE_BILLING = 'https://copilot.tencent.com/billing/meter';

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
  return cands.filter((p) => { try { return fs.existsSync(p); } catch { return false; } });
}

function jwtPayload(token) {
  try {
    const p = token.split('.')[1];
    const b = Buffer.from(p + '='.repeat(-p.length % 4), 'base64');
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
      return { token, uid, file: p, account: d.account || {} };
    } catch { /* try next */ }
  }
  return null;
}

async function apiCall(endpoint, login, base = API_BASE_CHECKIN, method = 'POST', body = '{}') {
  const resp = await fetch(base + endpoint, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'application/json',
      'Authorization': 'Bearer ' + login.token,
      'X-User-Id': login.uid,
    },
    body,
  });
  const json = await resp.json().catch(() => ({}));
  return { status: resp.status, json };
}

function mask(s, n = 8) { return s ? s.slice(0, n) + '…(' + s.length + '字符)' : '(空)'; }

async function cmdStatus() {
  const login = loadLogin();
  if (!login) { console.error('未找到 WorkBuddy 登录态，请先登录 WorkBuddy 客户端'); process.exit(2); }
  const { status, json } = await apiCall('/checkin-activity-status', login);
  if (status === 401) { console.error('登录态失效(401)，请重新登录 WorkBuddy'); process.exit(2); }
  if (json.code !== 0) { console.error('查询失败:', JSON.stringify(json)); process.exit(1); }
  const d = json.data || {};
  console.log(JSON.stringify({
    uid: login.uid,
    active: d.active,
    today_checked_in: d.today_checked_in,
    streak_days: d.streak_days,
    daily_credit: d.daily_credit,
    today_credit: d.today_credit,
    total_credits: d.total_credits,
    activity_name: d.activity_name,
    theme_name: d.theme_name,
    checkin_dates: d.checkin_dates,
  }, null, 2));
}

function isTrial(a) { return Number(a.CapacityType) === 4; }

function parseAccounts(accounts) {
  // 使用 Precise 字段匹配官方页面（如 86.97/1800）
  const getNum = (v) => Number(v ?? 0);
  const packages = accounts.map((a) => {
    const remain = getNum(a.CapacityRemainPrecise ?? a.CapacityRemain ?? 0);
    const size = getNum(a.CapacitySizePrecise ?? a.CapacitySize ?? 0);
    const used = getNum(a.CapacityUsedPrecise ?? a.CapacityUsed ?? 0);
    return {
      name: a.PackageName,
      type: isTrial(a) ? 'trial' : 'gift',
      typeName: isTrial(a) ? '个人体验版' : '权益赠送包',
      remain,
      size,
      used,
      cycle_start: a.CycleStartTime,
      cycle_end: a.CycleEndTime,
      deduction_end: a.DeductionEndTime,
      resource_id: a.ResourceId,
    };
  });
  packages.sort((a, b) => (a.cycle_end || '').localeCompare(b.cycle_end || ''));

  const gift = packages.filter((p) => p.type === 'gift');
  const trial = packages.filter((p) => p.type === 'trial');
  const sum = (arr, k) => arr.reduce((s, x) => s + x[k], 0);

  return {
    giftRemain: sum(gift, 'remain'),
    giftSize: sum(gift, 'size'),
    giftUsed: sum(gift, 'used'),
    trialRemain: sum(trial, 'remain'),
    trialSize: sum(trial, 'size'),
    trialUsed: sum(trial, 'used'),
    grandRemain: sum(packages, 'remain'),
    grandSize: sum(packages, 'size'),
    grandUsed: sum(packages, 'used'),
    packages,
  };
}

async function cmdQuota() {
  const login = loadLogin();
  if (!login) { console.error('未找到 WorkBuddy 登录态，请先登录 WorkBuddy 客户端'); process.exit(2); }
  const { status, json } = await apiCall('/get-user-resource', login, API_BASE_BILLING);
  if (status === 401) { console.error('登录态失效(401)，请重新登录 WorkBuddy'); process.exit(2); }
  if (json.code !== 0) { console.error('查询失败:', JSON.stringify(json)); process.exit(1); }
  const accounts = (json.data?.Response?.Data?.Accounts) || [];
  if (!accounts.length) { console.log(JSON.stringify({ uid: login.uid, gift_remain: 0, packages: [] }, null, 2)); return; }
  const s = parseAccounts(accounts);
  const giftPackages = s.packages.filter((p) => p.type === 'gift');
  const earliestExpire = giftPackages[0]?.cycle_end || '';
  console.log(JSON.stringify({
    uid: login.uid,
    gift_remain: Number(s.giftRemain.toFixed(4)),
    gift_size: Number(s.giftSize.toFixed(4)),
    gift_used: Number(s.giftUsed.toFixed(4)),
    trial_remain: Number(s.trialRemain.toFixed(4)),
    trial_size: Number(s.trialSize.toFixed(4)),
    grand_remain: Number(s.grandRemain.toFixed(4)),
    grand_size: Number(s.grandSize.toFixed(4)),
    earliest_expire: earliestExpire,
    packages: s.packages,
  }, null, 2));
}

async function cmdCheckin() {
  const login = loadLogin();
  if (!login) { console.error('未找到 WorkBuddy 登录态，请先登录'); process.exit(2); }
  const st = await apiCall('/checkin-activity-status', login);
  if (st.status === 401) { console.error('登录态失效(401)，请重新登录'); process.exit(2); }
  const already = (st.json.data && st.json.data.today_checked_in) || false;
  if (already) {
    console.log('今日已签到，无需重复。');
    console.log(JSON.stringify(st.json.data || {}, null, 2));
    process.exit(0);
  }
  const { status, json } = await apiCall('/daily-checkin', login);
  if (status === 401) { console.error('登录态失效(401)，请重新登录'); process.exit(2); }
  const code = json.code;
  if (code === 0) {
    const d = json.data || {};
    const before = (st.json.data && st.json.data.total_credits) || 0;
    console.log('签到成功: 本次 +' + (d.credit ?? 0) + ' 积分 | 连续 ' + (d.streak_days ?? 0) + ' 天 | 总积分 ' + (before + (d.credit ?? 0)));
    console.log(JSON.stringify(json, null, 2));
    process.exit(0);
  } else if (code === 10001 || /已签/.test(json.msg || '')) {
    console.log('今日已签到（接口确认）。');
    process.exit(0);
  } else {
    console.error('签到失败 code=' + code + ' msg=' + (json.msg || ''));
    process.exit(1);
  }
}

function cmdToken() {
  const login = loadLogin();
  if (!login) { console.error('未找到登录态'); process.exit(2); }
  console.log(JSON.stringify({
    uid: login.uid,
    nickname: login.account.nickname,
    type: login.account.type,
    token: mask(login.token),
    file: login.file,
  }, null, 2));
}

(async () => {
  const cmd = process.argv[2] || 'status';
  if (cmd === 'status') await cmdStatus();
  else if (cmd === 'quota') await cmdQuota();
  else if (cmd === 'checkin') await cmdCheckin();
  else if (cmd === 'token') cmdToken();
  else { console.error('未知命令: ' + cmd + '（支持 status | quota | checkin | token）'); process.exit(1); }
})();
