// WorkBuddy 账户中枢 — Tauri 桌面前端（调用 Rust Command 替代 fetch）
// 网络/账号操作全部走 Tauri invoke（无 Web 版）。
(function(){
  try { const el = document.getElementById('boot-log'); if (el) { el.textContent = '脚本入口已执行…'; el.className = 'show'; } }
  catch (e) {}
})();
window.onerror = function(msg, src, line, col, err) {
  const el = document.getElementById('boot-log');
  if (el) { el.textContent = '脚本错误 L' + line + ': ' + msg; el.className = 'show err'; }
  console.error('window.onerror', msg, 'line', line, err);
};
function rawInvoke(cmd, args = {}) {
  if (!window.__TAURI__ || !window.__TAURI__.core || typeof window.__TAURI__.core.invoke !== 'function') {
    return Promise.reject(new Error('Tauri API 尚未注入'));
  }
  return window.__TAURI__.core.invoke(cmd, args);
}
// 带超时保护的 invoke，避免启动时 IPC 未就绪导致无限挂起
function invokeWithTimeout(cmd, args = {}, ms = 8000) {
  return Promise.race([
    rawInvoke(cmd, args),
    new Promise((_, reject) => setTimeout(() => reject(new Error('Tauri invoke 超时: ' + cmd)), ms))
  ]);
}
const invoke = rawInvoke;

function $(id) { return document.getElementById(id); }
function bootLog(msg, cls = '') {
  let el = $('boot-log');
  if (!el) {
    // 若 index.html 未就绪，动态创建一个可见条
    el = document.createElement('div');
    el.id = 'boot-log';
    el.style.cssText = 'position:fixed;top:0;left:0;right:0;z-index:9999;padding:6px 10px;background:#1b212b;color:#e6e6e6;font-size:12px;border-bottom:1px solid #2a2f3a;';
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.className = 'show ' + cls;
  console.log('[boot]', msg);
}
// ===== 右上角历史消息中心（保存最近 100 条 toast/通知） =====
const NotifyCenter = {
  key: 'acc_hub_notifications_v1',
  max: 100,
  list() {
    try { return JSON.parse(localStorage.getItem(this.key)) || []; } catch (e) { return []; }
  },
  save(arr) {
    try { localStorage.setItem(this.key, JSON.stringify(arr.slice(0, this.max))); } catch (e) {}
    this.updateBadge();
  },
  push(msg, type) {
    const arr = this.list();
    arr.unshift({ t: Date.now(), msg: String(msg), type: type || '' });
    this.save(arr);
    this.render();
  },
  clear() {
    this.save([]);
    this.render();
  },
  updateBadge() {
    const n = this.list().length;
    const b = $('notify-badge');
    if (b) { b.textContent = n > 99 ? '99+' : String(n); b.style.display = n ? 'flex' : 'none'; }
  },
  render() {
    const list = $('notify-list'); if (!list) return;
    const arr = this.list();
    if (!arr.length) { list.innerHTML = '<div class="notify-empty">暂无消息</div>'; return; }
    list.innerHTML = arr.map((it, i) => {
      const time = new Date(it.t).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' });
      const cls = 'notify-item' + (it.type === 'err' ? ' err' : it.type === 'ok' ? ' ok' : '');
      return `<div class="${cls}" onclick="showNotifyDetail(${i})"><div class="t">${time}</div><div class="m">${escapeHtml(it.msg)}</div></div>`;
    }).join('');
  }
};
function toggleNotify() {
  const d = $('notify-dropdown'); if (!d) return;
  const show = d.classList.toggle('show');
  if (show) NotifyCenter.render();
}
function clearNotify() { NotifyCenter.clear(); }
function showNotifyDetail(i) {
  const arr = NotifyCenter.list(); const it = arr[i]; if (!it) return;
  $('notify-detail-time').textContent = new Date(it.t).toLocaleString('zh-CN');
  $('notify-detail-body').textContent = it.msg;
  $('notify-detail').classList.add('show');
}
function closeNotifyDetail() { $('notify-detail').classList.remove('show'); }
// 点击页面其他区域关闭下拉
document.addEventListener('click', function(e) {
  const w = $('notify-wrap');
  if (w && !w.contains(e.target)) { const d = $('notify-dropdown'); if (d) d.classList.remove('show'); }
});

function toast(msg, type) {
  // 右下角动态弹窗：支持多条堆叠，停留更久（5000ms），可点击关闭
  NotifyCenter.push(msg, type);
  let box = $('toast-box');
  if (!box) {
    box = document.createElement('div');
    box.id = 'toast-box';
    box.style.cssText = 'position:fixed;right:18px;bottom:18px;z-index:60;display:flex;flex-direction:column;gap:10px;align-items:flex-end;pointer-events:none;';
    document.body.appendChild(box);
  }
  const el = document.createElement('div');
  el.className = 'toast show' + (type ? ' ' + type : '');
  el.style.pointerEvents = 'auto';
  el.textContent = msg;
  box.appendChild(el);
  const timer = setTimeout(() => {
    el.style.opacity = '0';
    el.style.transform = 'translateY(8px)';
    setTimeout(() => el.remove(), 280);
  }, 9000);
  el.onclick = () => { clearTimeout(timer); el.remove(); };
  // 最多保留 4 条，超出移除最旧
  while (box.children.length > 4) box.firstChild.remove();
}
function shortUid(u) { return u ? u.slice(0, 8) + '…' : '(空)'; }

const num2 = v => Number(v ?? 0);
const isTrial = a => Number(a.CapacityType) === 4;
const pad = n => String(n).padStart(2, '0');
function fmt(ms) {
  if (!ms) return '—';
  const t = ms < 1e12 ? ms * 1000 : ms;
  const d = new Date(t); if (isNaN(d)) return String(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
function daysLeft(ms) {
  if (!ms) return null;
  const t = ms < 1e12 ? ms * 1000 : ms;
  const d = new Date(t); if (isNaN(d)) return null;
  return Math.ceil((d - Date.now()) / 86400000);
}
function escapeHtml(s) { return String(s).replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c])); }

// ===== 全局隐私分段工具 =====
// 把敏感值拆成 前缀(清晰) + 中段(默认模糊) + 后缀(清晰)，统一由全局眼睛控制。
// clear=true（眼睛已打开）时中段显示清晰；否则高斯模糊。
function privacy(val, opt) {
  opt = opt || {};
  const s = val === null || val === undefined ? '' : String(val);
  if (!s) return '<span class="psec">—</span>';
  // 短值或无需拆分的值直接返回、不做模糊
  if (opt.safe) return `<span class="psec">${escapeHtml(s)}</span>`;
  let pre, mid, suf;
  if (s.length <= 8) {
    // 过短整体模糊（如短 UID），露首1尾1
    pre = s.slice(0, 1); mid = s.slice(1, s.length - 1); suf = s.slice(s.length - 1);
    if (!mid) { pre = ''; suf = s; }
  } else {
    pre = s.slice(0, opt.head ?? 4);
    mid = s.slice(pre.length, s.length - (opt.tail ?? 4));
    suf = s.slice(s.length - (opt.tail ?? 4));
  }
  const midCls = keysRevealed ? 'mid show' : 'mid';
  return `<span class="psec"><span class="pre">${escapeHtml(pre)}</span><span class="${midCls}">${escapeHtml(mid)}</span><span class="suf">${escapeHtml(suf)}</span></span>`;
}

// ===== 侧边栏：账号管理（快照 / 切换） =====
let accountsCache = [];
function renderSidebar(j) {
  const cur = j.current_uid || (j.login && j.login.uid);
  if (j.login) {
    $('me-av').textContent = '👤';
    $('me-nm').innerHTML = privacy(j.login.nickname || j.login.uid, { head: 3, tail: 4, safe: false });
    $('me-id').innerHTML = privacy(j.login.uid, { head: 4, tail: 4, safe: false });
    $('me-type').textContent = j.login.type === 'personal' ? '个人账号' : (j.login.type || '—');
  }
  const al = $('acc-list'); al.innerHTML = '';
  const regs = j.registered_accounts || [];
  accountsCache = regs;
  showAccountList(regs, cur);

  const dl = $('dir-list'); dl.innerHTML = '';
  const dirs = (j.local_accounts && j.local_accounts.accounts) || [];
  if (!dirs.length) { dl.innerHTML = '<div class="empty">无本地目录</div>'; }
  dirs.forEach(d => {
    const div = document.createElement('div');
    div.className = 'dir' + (d.has_login ? ' has' : '') + (d.is_current ? ' active' : '');
    const tag = d.has_login ? '有登录态' : '空目录';
    div.innerHTML = `<span>📁 ${d.name}</span><span class="sz">${d.size} · ${tag}</span>`;
    div.title = d.name;
    div.onclick = () => selectDir(d, dirs);
    dl.appendChild(div);
  });
}

let selectedDir = null;
function selectAccount(a, all) {
  document.querySelectorAll('#acc-list .acc').forEach(el => {
    const id = el.querySelector('.s')?.textContent;
    el.classList.toggle('active', id === a.uid);
  });
  showAccountDetail(a, all);
}
function selectDir(d, all) {
  selectedDir = d.name;
  document.querySelectorAll('#dir-list .dir').forEach(el => el.classList.remove('active'));
  event.currentTarget.classList.add('active');
  $('kv-account').innerHTML =
    `<div class="item"><span class="k">目录名</span><span class="v">${d.name}</span></div>` +
    `<div class="item"><span class="k">磁盘占用</span><span class="v">${d.size}</span></div>` +
    `<div class="item"><span class="k">是否含登录态</span><span class="v">${d.has_login ? '是' : '否'}</span></div>` +
    `<div class="item"><span class="k">当前登录</span><span class="v">${d.is_current ? '是' : '否'}</span></div>`;
  $('memo-foryou').textContent = d.has_login ? '该目录含独立登录态文件（workbuddy-desktop.info）。' : '该目录无登录态，可能是历史残留或空账号。';
}

function showAccountDetail(a, all) {
  // 修复：AccountInfo 无 lastLogin 字段（旧逻辑 `all.find(x => x.lastLogin)` 恒为 undefined，
  // 且 `shortUid(a.uid)` 与全量 uid 永不相等 → isCur 恒 false，当前账号也误显示"需切换"）。
  // 改判：AccountInfo.current 字段（后端 list_accounts 已带）或当前登录 uid。
  const curUid = (window.__login && window.__login.uid) || ((all.find(x => x.current) || {}).uid);
  const isCur = a.current === true || (curUid && a.uid === curUid);
  const items = [
    ['昵称', a.nickname ? privacy(a.nickname, { head: 3, tail: 4 }) : '—'],
    ['UID', privacy(a.uid, { head: 4, tail: 4 })],
    ['手机号', privacy(a.phoneNumber || '—', { head: 3, tail: 4, safe: !a.phoneNumber })],
    ['账号类型', escapeHtml(a.type === 'personal' ? '个人' : '—')], ['最近登录', a.lastLogin ? '是' : '否'],
    ['创建者', a.isCreator ? '是' : '否'], ['管理员', a.isAdmin ? '是' : '否'],
  ];
  $('kv-account').innerHTML = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v}</span></div>`).join('');
  $('kv-jwt').innerHTML = '<span class="empty">已选账号非当前登录态，JWT/积分需切换 WorkBuddy 登录后查看</span>';
  $('memo-foryou').textContent = isCur ? '（当前登录账号，刷新全部可加载记忆画像）' : '（需在 WorkBuddy 中切换到该账号后才能读取其记忆画像）';
  $('pkg-body').innerHTML = '';
  $('raw-out').innerHTML = '已选中账号：' + (a.nickname ? privacy(a.nickname, { head: 3, tail: 4 }) : '') + ' (' + privacy(a.uid, { head: 4, tail: 4 }) + ')\n<span class="dim">' +
    (isCur ? '即当前登录态，点「刷新全部」加载完整数据。' : '非当前登录态。本工具只读当前登录态文件，无法直接拉取该账号远程数据（需先在 WorkBuddy 切换账号）。') + '</span>';
}

// ===== 渲染各板块（与 web 版一致） =====
function renderAccount(j) {
  if (!j || !j.uid) { $('kv-account').innerHTML = '<span class="empty">未找到登录态</span>'; return; }
  const items = [
    ['昵称', j.nickname ? privacy(j.nickname, { head: 3, tail: 4 }) : '—'],
    ['UID', privacy(j.uid, { head: 4, tail: 4 })],
    ['UIN', privacy(j.uin || '—', { head: 3, tail: 3, safe: !j.uin })],
    ['账号类型', escapeHtml(j.type === 'personal' ? '个人账号' : (j.type || '—'))],
    ['手机号', privacy(j.phoneNumber || '—', { head: 3, tail: 4, safe: !j.phoneNumber })],
    ['最近登录', j.lastLogin ? '是' : '否'], ['管理员', j.isAdmin ? '是' : '否'],
  ];
  $('kv-account').innerHTML = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v}</span></div>`).join('');
  const st = $('status-text'); if (st) st.innerHTML = '已读取 ' + (j.nickname ? privacy(j.nickname, { head: 3, tail: 4 }) : '') + ' · ' + privacy(j.uid, { head: 4, tail: 4 });
}
function renderJwt(j) {
  if (!j || j.error) { $('kv-jwt').innerHTML = '<span class="empty">解析失败</span>'; $('jwt-ring').style.setProperty('--p', 0); return; }
  const items = [
    ['签发者', escapeHtml(j.issuer || '—')],
    ['主题', privacy(j.subject, { head: 6, tail: 4 })],
    ['受众', privacy(j.audience, { head: 4, tail: 4 })],
    ['授权方', escapeHtml(j.azp || '—')], ['邮箱验证', j.email_verified ? '是' : '否'],
    ['用户名', privacy(j.preferred_username, { head: 3, tail: 3, safe: !j.preferred_username })],
    ['签发时间', escapeHtml(j.issued_at || '—')], ['过期时间', escapeHtml(j.expires_at || '—')],
  ];
  $('kv-jwt').innerHTML = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v || '—'}</span></div>`).join('');
  const days = j.remaining_days ?? 0;
  const pct = Math.max(0, Math.min(100, Math.round(days / 90 * 100)));
  $('jwt-ring').style.setProperty('--p', pct);
  $('jwt-ring').style.setProperty('--c', days < 15 ? 'var(--red)' : (days < 30 ? 'var(--amber)' : 'var(--green)'));
  $('jwt-days').textContent = days;
  $('ov-jwt').textContent = days + ' 天';
  $('ov-jwt-m').textContent = '过期 ' + (j.expires_at || '—').slice(0, 10);
}
function renderEnv(j) {
  if (!j) { $('kv-env').innerHTML = '<span class="empty">无数据</span>'; return; }
  const items = [
    ['WorkBuddy 版本', j.version || '—'], ['构建号', j.build || '—'],
    ['安装大小', j.appSize || '—'], ['系统平台', j.platform], ['架构', j.arch], ['Node', j.node],
  ];
  $('kv-env').innerHTML = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v || '—'}</span></div>`).join('');
}
function parseQuota(body) {
  const data = body?.data?.Response?.Data;
  if (!data || !data.Accounts) return null;
  const accs = data.Accounts;
  const pkgs = accs.map(a => ({
    name: a.PackageName, resource_id: a.ResourceId, trial: isTrial(a),
    remain: num2(a.CapacityRemainPrecise ?? a.CapacityRemain),
    used: num2(a.CapacityUsedPrecise ?? a.CapacityUsed),
    size: num2(a.CapacitySizePrecise ?? a.CapacitySize),
    cycle_end: a.CycleEndTime, deduction_end: a.DeductionEndTime,
  }));
  const gift = pkgs.filter(p => !p.trial);
  const trial = pkgs.filter(p => p.trial);
  const sum = (arr, k) => arr.reduce((s, x) => s + x[k], 0);
  let soonest = null;
  for (const a of gift) { const dl = daysLeft(a.deduction_end); if (dl === null) continue; if (!soonest || dl < soonest.dl) soonest = { dl, a }; }
  return {
    pkgs, gift, trial,
    giftRemain: sum(gift, 'remain'), giftUsed: sum(gift, 'used'), giftSize: sum(gift, 'size'),
    trialRemain: sum(trial, 'remain'),
    grandRemain: sum(pkgs, 'remain'), grandUsed: sum(pkgs, 'used'), grandSize: sum(pkgs, 'size'),
    soonest, usePct: sum(pkgs, 'size') > 0 ? Math.round(sum(pkgs, 'used') / sum(pkgs, 'size') * 100) : 0,
  };
}
// 后端标准化解析（packages + 汇总）适配为前端渲染所需的 q 结构
function adaptParsed(p) {
  const pkgs = (p.packages || []).map(x => ({
    name: x.name, resource_id: x.resource_id, trial: x.trial,
    remain: x.remain, used: x.used, size: x.size,
    cycle_end: x.cycle_end, deduction_end: x.deduction_end, is_unlimited: x.is_unlimited
  }));
  const gift = pkgs.filter(x => !x.trial), trial = pkgs.filter(x => x.trial);
  const sum = (a, k) => a.reduce((s, x) => s + (x[k] || 0), 0);
  let soonest = null;
  for (const a of gift) { const dl = daysLeft(a.deduction_end); if (dl === null) continue; if (!soonest || dl < soonest.dl) soonest = { dl, a }; }
  return {
    pkgs, gift, trial,
    giftRemain: sum(gift, 'remain'), giftUsed: sum(gift, 'used'), giftSize: sum(gift, 'size'),
    trialRemain: sum(trial, 'remain'),
    grandRemain: sum(pkgs, 'remain'), grandUsed: sum(pkgs, 'used'), grandSize: sum(pkgs, 'size'),
    soonest, usePct: (p.usePct != null ? p.usePct : 0), hasUnlimited: p.hasUnlimited
  };
}
function renderQuota(payload) {
  // 优先用后端标准化解析（parsed：含别名兼容/合并/企业不限量），失败回退前端 parseQuota 解析原始 body
  let q;
  if (payload && payload.parsed && Array.isArray(payload.parsed.packages)) {
    q = adaptParsed(payload.parsed);
  } else {
    q = parseQuota((payload && payload.body) ? payload.body : payload);
  }
  if (!q) return;
  const { pkgs, gift, trial, giftRemain, giftUsed, giftSize, trialRemain, grandRemain, grandUsed, grandSize, soonest, usePct } = q;
  $('ov-gift').textContent = giftRemain.toFixed(2);
  $('ov-gift-m').textContent = `已用 ${giftUsed.toFixed(2)} / ${giftSize.toFixed(2)} · ${gift.length} 包`;
  $('ov-trial').textContent = trialRemain.toFixed(2);
  $('ov-trial-m').textContent = `体验版 · ${trial.length} 个`;
  $('ov-grand').textContent = grandRemain.toFixed(2);
  $('ov-grand-m').textContent = `已用 ${grandUsed.toFixed(2)} / ${grandSize.toFixed(2)}`;
  $('q-use').textContent = usePct + '%';
  $('q-use-m').textContent = `${grandUsed.toFixed(2)} / ${grandSize.toFixed(2)}`;
  $('q-count').textContent = pkgs.length;
  $('q-expire').textContent = soonest ? (soonest.dl <= 0 ? '已过期' : soonest.dl + ' 天') : '长期';
  const track = $('axis-track');
  const dots = gift.map(p => {
    const dl = daysLeft(p.deduction_end);
    if (dl === null) return '';
    const pos = Math.max(6, Math.min(94, 6 + (dl / 90) * 88));
    const near = dl <= 30;
    const label = dl <= 0 ? '已过期' : (dl + '天');
    return `<div class="axis-dot ${near ? 'near' : ''}" style="left:${pos}%;"><div class="pt"></div><span class="d">${label}</span></div>`;
  }).join('');
  track.querySelectorAll('.axis-dot').forEach(n => n.remove());
  track.insertAdjacentHTML('beforeend', dots);
  const tb = $('pkg-body'); tb.innerHTML = '';
  for (const a of pkgs) {
    const pct = a.size > 0 ? Math.round(a.used / a.size * 100) : 0;
    const dl = daysLeft(a.deduction_end);
    const pill = a.trial ? '<span class="pill trial">体验版</span>' : '<span class="pill gift">赠送包</span>';
    const exp = (dl !== null) ? (dl <= 30 ? `<span class="soon">${fmt(a.deduction_end)} (${dl}天)</span>` : fmt(a.deduction_end)) : '—';
    tb.appendChild(row([
      `${a.name || '—'}<div class="meta" style="color:var(--muted);font-size:10px;">${a.resource_id || ''}</div>`, pill,
      a.remain.toFixed(2), a.used.toFixed(2), a.size.toFixed(2),
      a.cycle_end || '—', exp,
      `<div class="bar"><i style="width:${pct}%"></i></div><span class="meta" style="color:var(--muted);font-size:10px;">${pct}%</span>`
    ]));
  }
  // 30 天内到期：只看「还有剩余额度（remain>0）」的包，避免把已 100% 用完的每日赠送包全罗列；并按 resource_id 去重
  const seenRid = new Set();
  const soon = gift.filter(p => {
    const dl = daysLeft(p.deduction_end);
    if (dl === null || dl > 30) return false;
    if (!(p.remain > 0.004)) return false;          // 剩余≤0 的已用尽
    if (seenRid.has(p.resource_id)) return false;  // 同资源去重
    seenRid.add(p.resource_id);
    return true;
  }).sort((x, y) => daysLeft(x.deduction_end) - daysLeft(y.deduction_end));
  $('expire-note').innerHTML = soon.length ? '⏰ 30 天内到期（剩余额度>0）：' + soon.map(p => `${p.name}（${daysLeft(p.deduction_end)}天，${fmt(p.deduction_end)}，余 ${p.remain.toFixed(2)}）`).join('；') : '近期无待用套餐到期。';
  window.__quota = q;
  // Batch C：趋势记录 + 预算条（单账号视图）
  const _quid = payload && payload.login && payload.login.uid;
  if (_quid) { recordTrend(_quid, q.grandRemain); renderTrend(_quid); }
  renderBudgetBar(q);
}
function row(cells) { const tr = document.createElement('tr'); tr.innerHTML = cells.map((c, i) => `<td class="${i >= 2 && i <= 4 ? 'num' : ''}">${c}</td>`).join(''); return tr; }
function renderCheckin(body) {
  const d = body?.data;
  if (!d) return;
  $('ck-active').textContent = d.active ? '是' : '否';
  $('ck-today').textContent = d.today_checked_in ? '已签 ✓' : '未签';
  $('ck-today').style.color = d.today_checked_in ? 'var(--green)' : 'var(--fg)';
  $('ck-streak').textContent = (d.streak_days || 0) + ' 天';
  $('ck-credit').textContent = (d.today_credit || 0);
  const dates = (d.checkin_dates || []).slice(-16);
  // 修复：旧代码用 toISOString()（UTC 日期），北京时间 0-8 点会与本地日期错位一天。
  const now = new Date();
  const today = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  $('ck-timeline').innerHTML = dates.length ? dates.map(dt => {
    const isToday = dt === today;
    return `<span class="chip ${isToday ? 'today' : 'done'}">${dt.slice(5)}</span>`;
  }).join('') : '<span class="empty">暂无记录</span>';
}
function renderMemory(body) {
  const d = body?.data;
  if (!d) { $('kv-memory').innerHTML = '<span class="empty">无数据</span>'; return; }
  const items = [
    ['用户 ID', privacy(d.user_id, { head: 4, tail: 4, safe: !d.user_id })],
    ['用户名', privacy(d.user_name, { head: 3, tail: 3, safe: !d.user_name })],
    ['更新时间', (d.updated_at || d.updatedAt || '—')], ['版本', d.version ?? '—'],
  ];
  $('kv-memory').innerHTML = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v || '—'}</span></div>`).join('');
  const raw = d.foryou_prompt || d.memory || '';
  const ptCls = keysRevealed ? 'privacy-text show' : 'privacy-text';
  $('memo-foryou').innerHTML = raw ? `<span class="${ptCls}">${escapeHtml(raw)}</span>` : '(空)';
  const box = $('memory-sections'); box.innerHTML = '';
  const bodyHtml = txt => `<div class="sec-body"><span class="${ptCls}">${escapeHtml(txt)}</span></div>`;
  if (raw) {
    const parts = raw.split(/^##\s+/m).filter(Boolean);
    parts.forEach(p => {
      const nl = p.indexOf('\n');
      const title = (nl > 0 ? p.slice(0, nl) : p).trim();
      const content = (nl > 0 ? p.slice(nl + 1) : '').trim();
      const det = document.createElement('details');
      det.className = 'sec'; det.open = false;
      det.innerHTML = `<summary>${escapeHtml(title)}</summary>${bodyHtml(content)}`;
      box.appendChild(det);
    });
    if (!parts.length) {
      const det = document.createElement('details'); det.className = 'sec'; det.open = false;
      det.innerHTML = `<summary>记忆内容</summary>${bodyHtml(raw)}`;
      box.appendChild(det);
    }
  } else { box.innerHTML = '<span class="empty">无记忆画像</span>'; }
  window.__memoryRaw = raw;
}

// ===== 导出报告（与 web 版一致） =====
function buildReport() {
  const L = window.__login || {};
  const q = window.__quota || {};
  const lines = [];
  lines.push('# WorkBuddy 账户快照');
  lines.push(`生成时间：${new Date().toLocaleString()}`);
  lines.push('');
  lines.push('## 账户');
  lines.push(`- 昵称/手机号：${L.nickname || '—'}`);
  lines.push(`- UID：${L.uid || '—'}`);
  lines.push(`- UIN：${L.uin || '—'}`);
  lines.push(`- 类型：${L.type === 'personal' ? '个人账号' : '—'}`);
  lines.push(`- 登录态剩余：${($('jwt-days').textContent || '—')} 天（过期 ${($('ov-jwt-m').textContent || '').replace('过期 ', '')}）`);
  if (q.pkgs) {
    lines.push(''); lines.push('## 积分额度');
    lines.push(`- 权益赠送包剩余：${q.giftRemain.toFixed(2)}（已用 ${q.giftUsed.toFixed(2)} / ${q.giftSize.toFixed(2)}，${q.gift.length} 包）`);
    lines.push(`- 体验版剩余：${q.trialRemain.toFixed(2)}（${q.trial.length} 个）`);
    lines.push(`- 全部剩余：${q.grandRemain.toFixed(2)}（已用 ${q.grandUsed.toFixed(2)} / ${q.grandSize.toFixed(2)}）`);
    lines.push(`- 总使用率：${q.usePct}%`);
    lines.push(`- 最早到期：${($('q-expire').textContent || '—')}`);
    lines.push('- 套餐明细：');
    q.pkgs.forEach(p => {
      const dl = daysLeft(p.deduction_end);
      lines.push(`  - ${p.name} [${p.trial ? '体验版' : '赠送包'}] 剩余 ${p.remain.toFixed(2)} / ${p.size.toFixed(2)}，抵扣到期 ${p.deduction_end || '—'}${dl !== null ? ` (${dl}天)` : ''}`);
    });
  }
  const ck = window.__checkin || {};
  if (ck.data) {
    lines.push(''); lines.push('## 每日签到');
    lines.push(`- 活动进行中：${ck.data.active ? '是' : '否'}`);
    lines.push(`- 今日：${ck.data.today_checked_in ? '已签 ✓' : '未签'}`);
    lines.push(`- 连续：${ck.data.streak_days || 0} 天 · 今日得 ${ck.data.today_credit || 0}`);
    const dates = (ck.data.checkin_dates || []).slice(-8);
    if (dates.length) lines.push(`- 最近：${dates.join('、')}`);
  }
  const raw = window.__memoryRaw || '';
  if (raw) {
    lines.push(''); lines.push('## AI 记忆画像摘要');
    const parts = raw.split(/^##\s+/m).filter(Boolean);
    if (parts.length) parts.forEach(p => { const nl = p.indexOf('\n'); lines.push(`- ${nl > 0 ? p.slice(0, nl).trim() : p.trim()}`); });
    else lines.push(raw.slice(0, 200));
  }
  lines.push(''); lines.push('---'); lines.push('由 WorkBuddy 账户中枢（桌面端）生成');
  return lines.join('\n');
}
function openReport() { $('report-out').value = buildReport(); $('report-modal').classList.add('show'); }
function closeReport() { $('report-modal').classList.remove('show'); }
function copyReport() {
  const t = $('report-out'); t.select();
  navigator.clipboard?.writeText(t.value).then(
    () => { const b = document.querySelector('#report-modal .modal-foot button:last-child'); const o = b.textContent; b.textContent = '已复制 ✓'; setTimeout(() => b.textContent = o, 1500); },
    () => { document.execCommand('copy'); }
  );
}
if ($('report-modal')) $('report-modal').addEventListener('click', e => { if (e.target.id === 'report-modal') closeReport(); });

// ===== 账号操作 =====
// 备份所有已登记账号：当前账号全新备份（含最新登录态），其余账号归档其保存的快照。
// 登录态默认自动保存（App 启动时已调用 ensure_snapshot），无需手动「记住」。
async function backupAll() {
  try {
    toast('正在备份所有账号…');
    const rows = await invoke('backup_all');
    if (!rows || !rows.length) { toast('未发现可备份的账号'); return; }
    const okN = rows.filter(r => r.ok).length;
    const fails = rows.filter(r => !r.ok);
    toast('已备份 ' + okN + '/' + rows.length + ' 个账号' + (fails.length ? '，' + fails.length + ' 个失败（多为未保存过登录态）' : ''));
  } catch (e) { toast('备份失败: ' + e); }
}

// 启动时自动保存当前账号登录态（默认自动保存，无需手动操作）
async function ensureSnapshot() {
  try { await invoke('ensure_snapshot'); } catch (e) { /* 无登录态时忽略 */ }
}

// 手动保存「当前官方登录账号」的登录态（accessToken 等）到中枢保险库，
// 使其可切换/可备份。适用于：在官方客户端刚登录某账号后，点一下即可固化。
async function saveCurrentLogin() {
  try {
    const m = await invoke('snapshot_current');
    if (!m || m.uid == null) { toast('未读取到当前登录账号'); return; }
    toast('已保存当前账号登录态：' + (m.uid || '') + (m.auth_included ? '（含 token）' : ''));
    refreshAccounts();
  } catch (e) { toast('保存失败: ' + e); }
}

// ===== 账号切换 =====
// 切换中的账号，防止重复点击（切换较慢，重复点会叠加 IPC）
let switchingUid = null;
async function switchTo(uid) {
  // 一键切换：切换前自动备份当前账号（含 workbuddy.db 会话库）→ 写入目标登录态（真实 token + 真实 uid）
  // → 把源账号在 workbuddy.db 里的会话/自动化归属搬迁到目标账号名下（"搬"语义，用户核心诉求）
  // → 重启 WorkBuddy 以新登录态生效。切换前若 WorkBuddy 正在运行，先弹确认框（避免中断任务）。
  if (switchingUid) { toast('正在切换 ' + shortUid(switchingUid) + '，请稍候…'); return; }
  try {
    // 复用最近一次 list_accounts 已带回的 workbuddy_running 状态，避免切换前再发一次 IPC。
    let wbRunning = false;
    const cached = window.__accountsMeta;
    if (cached && typeof cached.workbuddy_running === 'boolean') {
      wbRunning = cached.workbuddy_running;
    } else {
      try { wbRunning = await invoke('app_running'); } catch (e) {}
    }

    if (wbRunning) {
      // 软件内确认框：提示关闭 WorkBuddy 会中断可能在跑的任务
      const ok = await confirmSwitchRisk(uid);
      if (!ok) { toast('已取消切换'); return; }
    }

    switchingUid = uid;
    try { showAccountList(accountsCache, (window.__login || {}).uid); } catch (e) {}
    toast('正在切换 ' + shortUid(uid) + '…');
    const r = await invoke('switch_account', { uid });
    if (r) {
      toast('已切换到 ' + shortUid(r.uid) + ' ✅ 正在重启 WorkBuddy 生效…', 'ok');
      // 切换成功且 WorkBuddy 当前无任务运行（已被后端优雅退出或本就未运行），则自动重启 WorkBuddy 生效
      setTimeout(async () => {
        try { await invoke('restart_workbuddy'); } catch (e) { toast('重启 WorkBuddy 失败: ' + e, 'err'); }
      }, 1200);
      refreshAccounts();
      setTimeout(() => loadAll(), 1500);
    }
  } catch (e) {
    toast('切换失败: ' + e, 'err');
  } finally {
    switchingUid = null;
  }
}

// 软件内确认弹窗（自定义样式，提示关闭 WorkBuddy 的中断风险）
let switchResolve = null;
function confirmSwitchRisk(uid) {
  return new Promise(res => {
    $('sw-risk-uid').textContent = shortUid(uid);
    $('sw-risk-status').textContent = '检测到 WorkBuddy 正在运行。切换将先关闭 WorkBuddy（如有任务正在生成/下载会被中断），再自动重启切换账号。\n\n⚠️ 切换会把「当前账号」在会话库里的对话/自动化搬迁到目标账号名下（"搬"语义）：切过去后能看到并继续原对话，但源账号视角下这些内容不再可见；来回切换会让会话在两个账号间流动。';
    $('sw-risk-modal').classList.add('show');
    switchResolve = res;
  });
}
function confirmSwitchYes() { $('sw-risk-modal').classList.remove('show'); if (switchResolve) { switchResolve(true); switchResolve = null; } }
function confirmSwitchNo() { $('sw-risk-modal').classList.remove('show'); if (switchResolve) { switchResolve(false); switchResolve = null; } }
// （banner 死 UI 已移除：切换后由 switchTo 自动调 restart_workbuddy，无需手动入口）

async function refreshAccounts() {
  let data;
  try { data = await invoke('list_accounts'); } catch { return; }
  // 缓存 workbuddy_running 等元数据，供 switchTo 判断是否需要确认框（省一次 IPC）
  if (data) window.__accountsMeta = data;
  // list_accounts 返回完整账号清单（登记表 + vault 兜底 + 当前），
  // 这里把 has_snapshot 合并进 accountsCache，并且如果清单比此前多出了账号
  // （例如后端从此前"单账号"进阶到"双账号"枚举），主动重绘侧边栏让它们立即可见，
  // 而不必等用户手动点「刷新全部」。
  const freshAccs = data.accounts || [];
  if (freshAccs.length) {
    // 用新清单重算账号列表（保留旧 UI 数据，仅补齐 has_snapshot）
    const merged = [];
    const byUid = new Map();
    accountsCache.forEach(a => byUid.set(a.uid, a));
    freshAccs.forEach(f => {
      const old = byUid.get(f.uid);
      merged.push(old ? { ...old, has_snapshot: f.has_snapshot } : {
        uid: f.uid, nickname: f.nickname, has_snapshot: f.has_snapshot,
      });
    });
    // 当前账号保持 active（若在清单里）
    const j = window.__login || {};
    const cur = j.uid;
    const changed = merged.length !== accountsCache.length
      || merged.some(m => !accountsCache.find(x => x.uid === m.uid));
    if (changed) { accountsCache = merged; showAccountList(merged, cur); }
    else { accountsCache.forEach(a => {
      const hit = freshAccs.find(x => x.uid === a.uid);
      if (hit) a.has_snapshot = hit.has_snapshot;
    }); }
  }
}

// 复用 renderSidebar 里的账号侧栏绘制逻辑，但只重绘账号列表（不重设其它区块）
function showAccountList(accs, cur) {
  const al = $('acc-list'); if (!al) return;
  al.innerHTML = '';
  if (!accs.length) { al.innerHTML = '<div class="empty">无已登记账号</div>'; return; }
  accs.forEach(a => {
    const div = document.createElement('div');
    div.className = 'acc' + (a.uid === cur ? ' active' : '');
    const initial = (a.nickname || a.uid || '?').slice(0, 2);
    const isCur = a.uid === cur;
    const snapped = a.has_snapshot;
    const isSwitching = switchingUid === a.uid;
    const badge = isCur
      ? '<span class="cur">当前</span>'
      : (snapped
        ? `<button class="mini" ${isSwitching ? 'disabled style="opacity:.5;cursor:default;"' : ''} onclick="event.stopPropagation();switchTo('${a.uid}')">${isSwitching ? '切换中…' : '切换'}</button>`
        : '<span class="mini" style="background:var(--line);color:var(--muted);padding:3px 9px;border-radius:6px;font-size:10.5px;" title="该账号尚未在 WorkBuddy 中登录保存过登录态，无法直接切换">未登录</span>');
    const meta = getMeta(a.uid);
    const starBtn = `<button class="mini star ${meta.star ? 'on' : ''}" title="星标" onclick="event.stopPropagation();toggleStar('${a.uid}')">${meta.star ? '★' : '☆'}</button>`;
    const tagBtn = `<button class="mini" title="标签：${meta.tags.join(', ') || '（点击设置）'}" onclick="event.stopPropagation();openTagInput('${a.uid}')">🏷</button>`;
    div.dataset.uid = a.uid;
    div.innerHTML = `
      <div class="dot">${initial}</div>
      <div class="info"><div class="n">${a.nickname ? privacy(a.nickname, { head: 3, tail: 4 }) : '(无昵称)'}</div><div class="s">${privacy(a.uid, { head: 4, tail: 4 })}</div></div>
      ${badge}
      <div class="acc-actions">${starBtn}${tagBtn}</div>`;
    div.onclick = () => selectAccount(a, accs);
    al.appendChild(div);
  });
  accountsCache = accs;
}

// ===== 历史备份页 =====
let backupsCache = [];           // [{uid, ts, file_count, auth_included, bytes, local_files, is_latest}]
function escBack(ts) { return String(ts).replace(/[^0-9a-zA-Z_-]/g, '_'); }
function fmtBytes(b) {
  if (!b) return '0 B';
  if (b < 1024) return b + ' B';
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB';
  return (b / 1048576).toFixed(2) + ' MB';
}
function fmtTs(ts) {
  if (!ts) return ts || '—';
  const n = /^[0-9]+$/.test(ts) ? Number(ts) : NaN;
  if (!isNaN(n) && ts.length >= 10) {
    // ts 为毫秒（13 位）或秒（10 位），统一转 Date
    const d = new Date(ts.length >= 13 ? n : n * 1000);
    if (!isNaN(d)) return `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }
  return ts;
}
async function openBackups() {
  try { backupsCache = await invoke('list_backups'); }
  catch (e) { backupsCache = []; toast('读取备份失败: ' + e); }
  $('bk-modal').classList.add('show');
  renderBackups();
}

async function renderBackups() {
  const box = $('bk-list');
  box.innerHTML = '';
  if (!backupsCache || !backupsCache.length) {
    box.innerHTML = '<div class="empty">暂无历史备份。\n点侧边栏当前账号的「快照当前」，或切换账号时系统会自动为当前账号生成备份。</div>';
    return;
  }
  // 按 uid 分组
  const groups = {};
  backupsCache.forEach(b => { (groups[b.uid] = groups[b.uid] || []).push(b); });
  Object.keys(groups).forEach(uid => {
    const items = groups[uid].sort((a, b) => (b.ts > a.ts ? 1 : -1));
    const sec = document.createElement('div');
    sec.style.marginBottom = '14px';
    sec.innerHTML = `<div style="font-size:11px;color:var(--muted);margin:6px 2px 6px;font-weight:600;">账号 ${privacy(uid, { head: 4, tail: 4, safe: false })} · ${items.length} 份</div>`;
    const list = document.createElement('div');
    items.forEach(b => {
      const row = document.createElement('div');
      row.className = 'bk-row';
      row.innerHTML = `
        <span class="bk-dot"></span>
        <div class="bk-info">
          <div class="bk-t">${fmtTs(b.ts)}${b.is_latest ? ' <span class="badge bk-latest">最新</span>' : ''}</div>
          <div class="bk-s">${b.file_count ?? 0} 文件${b.auth_included ? ' · 含登录态' : ''} · 共 ${fmtBytes(b.bytes)}</div>
        </div>
        <button class="mini secondary" onclick="event.stopPropagation();openBackupDetail('${escBack(b.uid)}','${escBack(b.ts)}')">详情</button>`;
      list.appendChild(row);
    });
    sec.appendChild(list);
    box.appendChild(sec);
  });
}

async function openBackupDetail(uid, ts) {
  let meta;
  try { meta = await invoke('backup_detail', { uid, ts }); }
  catch (e) { toast('读取备份详情失败: ' + e); return; }
  if (!meta) return;
  const head = $('bk-detail-head');
  head.innerHTML = `备份详情 · ${privacy(uid, { head: 4, tail: 4, safe: false })} @ ${fmtTs(ts)}${meta.is_latest ? ' <span class="badge bk-latest">最新</span>' : ''}`;
  const kv = [
    ['备份时间', fmtTs(meta.ts)],
    ['UID', privacy(meta.uid, { head: 4, tail: 4, safe: false })],
    ['文件数量', (meta.file_count ?? 0) + ' 个'],
    ['含登录态', meta.auth_included ? '是（auth.info）' : '否'],
    ['总大小', fmtBytes(meta.bytes)],
  ];
  const kvBox = $('bk-detail-kv');
  kvBox.innerHTML = kv.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v}</span></div>`).join('');
  const tree = (meta.local_files || []).join('\n') || '（无文件）';
  const treeBox = $('bk-detail-tree');
  treeBox.innerHTML = escapeHtml(tree);
  $('bk-detail-modal').classList.add('show');
}

function closeBackups() { $('bk-modal').classList.remove('show'); }
function closeBackupDetail() { $('bk-detail-modal').classList.remove('show'); }
if ($('bk-modal')) $('bk-modal').addEventListener('click', e => { if (e.target.id === 'bk-modal') closeBackups(); });
if ($('bk-detail-modal')) $('bk-detail-modal').addEventListener('click', e => { if (e.target.id === 'bk-detail-modal') closeBackupDetail(); });

// ===== 宠物旅行（buddy travel） =====
let buddyLocations = [];       // 可选地点列表 [{location_id,name,hour,reward,desc}]
let buddyLog = [];             // 本次会话操作日志 [{t,msg,cls}]
let buddyTimer = null;         // 自动轮询定时器

function buddyLogAdd(msg, cls) {
  buddyLog.unshift({ t: new Date(), msg, cls: cls || 'info' });
  const box = $('buddy-log');
  if (!box) return;
  const line = document.createElement('div');
  line.className = 'line';
  const t = `${pad(buddyLog[0].t.getHours())}:${pad(buddyLog[0].t.getMinutes())}:${pad(buddyLog[0].t.getSeconds())}`;
  line.innerHTML = `<span class="t">${t}</span><span class="${cls || 'info'}">${escapeHtml(msg)}</span>`;
  box.prepend(line);
  while (box.children.length > 60) box.lastChild?.remove();
}
function buddyLogInit() {
  const box = $('buddy-log');
  if (!box) return;
  box.innerHTML = '';
  (buddyLog || []).forEach(l => {
    const line = document.createElement('div');
    line.className = 'line';
    const t = `${pad(l.t.getHours())}:${pad(l.t.getMinutes())}:${pad(l.t.getSeconds())}`;
    line.innerHTML = `<span class="t">${t}</span><span class="${l.cls || 'info'}">${escapeHtml(l.msg)}</span>`;
    box.appendChild(line);
  });
  if (!buddyLog.length) box.innerHTML = '<span class="empty">暂无记录。点击「🔄 刷新」读取宠物状态。</span>';
}

// 从 status 响应中安全取 data（body 可能是 JSON 字符串或已解析对象）
function buddyData(body, field) {
  if (body === null || body === undefined) return undefined;
  const d = (typeof body === 'object' && 'data' in body) ? body.data : body;
  return d && field ? d[field] : d;
}
// 安全解析 body：可能已是对象，也可能是 JSON 字符串；解析失败原样返回，不抛异常
// （旧代码在多处裸 JSON.parse，后端返回非 JSON 文本时会直接 throw 使整个面板报错）
function parseBody(b) {
  if (typeof b === 'string') { try { return JSON.parse(b); } catch { return b; } }
  return b;
}
function setBuddyState(state, loc, desc, arriveAt) {
  const tag = $('buddy-state-tag'); if (!tag) return;
  const av = $('buddy-avatar');
  const map = {
    idle: ['休息中', 'idle', '🐱', '宠物在家休息，可以派它出去旅行赚积分。'],
    traveling: ['旅行中', 'traveling', '🧳', desc || '宠物正在旅途中，即将到达目的地。'],
    arrived: ['已到达 🎉', 'arrived', '🎁', desc || '宠物已到达目的地，可以领取旅行奖励了！'],
  };
  const m = map[state] || map.idle;
  tag.textContent = m[0]; tag.className = 'st-tag ' + m[1];
  av.textContent = m[2]; av.className = 'buddy-avatar ' + (m[1] === 'idle' ? '' : m[1]);
  $('buddy-desc').textContent = desc || m[3];
  $('buddy-countdown').style.display = (state === 'traveling' && arriveAt) ? 'flex' : 'none';
  // 按钮可用性
  $('btn-buddy-depart').disabled = state !== 'idle';
  if (state === 'idle') { $('btn-buddy-depart').textContent = '📤 派出宠物'; }
  const claimBtn = $('btn-buddy-claim');
  if (state === 'arrived') { claimBtn.disabled = false; claimBtn.innerHTML = '🎁 领取奖励'; }
  else { claimBtn.disabled = true; claimBtn.innerHTML = '🎁 领取奖励'; }
}
function buddyCountdownTick() {
  const c = $('buddy-countdown'); if (!c || c.style.display === 'none') return;
  const at = parseInt(c.dataset.at || 0, 10);
  if (!at) return;
  const remain = at - Date.now();
  if (remain <= 0) { c.textContent = '即将到达 ⌛'; loadBuddy(); return; }
  const s = Math.floor(remain / 1000);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  c.textContent = `⏱ 还有 ${h}时 ${m}分 ${sec}秒 到达`;
}

async function loadBuddy() {
  let ok = false;
  try {
    const [st, cfg] = await Promise.all([
      invoke('buddy_status').catch(e => ({ error: String(e) })),
      invoke('buddy_config').catch(e => ({ error: String(e) })),
    ]);
    if (st && st.error) { buddyLogAdd('读取状态失败: ' + st.error, 'err'); return false; }
    // 地点配置
    if (cfg && !cfg.error && cfg.status === 200) {
      const cfgBody = parseBody(cfg.body);
      const rawLocs = buddyData(cfgBody, 'locations') || buddyData(cfgBody, 'config') || [];
      buddyLocations = (Array.isArray(rawLocs) ? rawLocs : []).map(l => {
        const hMin = l.duration_hours_min ?? l.hour_min ?? l.duration_hour ?? 0;
        const hMax = l.duration_hours_max ?? l.hour_max ?? l.duration_hours ?? hMin;
        const rMin = l.reward_credit_min ?? l.reward_min ?? l.reward_credit ?? 0;
        const rMax = l.reward_credit_max ?? l.reward_max ?? rMin;
        return {
          id: String(l.location_id ?? l.id ?? ''),
          name: l.name || l.location_name || '',
          hour: hMin, hourMax: hMax,
          reward: rMin, rewardMax: rMax,
          desc: l.description || l.desc || '',
        };
      }).filter(l => l.id !== '' && l.id !== null && l.id !== undefined);
      renderBuddyLocs();
    }
    // 状态
    if (st.status === 200) {
      const sBody = parseBody(st.body);
      const d = buddyData(sBody);
      renderBuddy(d);
      ok = true;
      refreshBuddyAll();   // 当前账号宠物加载完后，顺带拉取全部账号宠物状态
    } else if (st.status && st.status !== 200) {
      buddyLogAdd('状态接口返回 ' + st.status, 'err');
    }
  } catch (e) {
    buddyLogAdd('加载宠物失败: ' + e, 'err');
    toast('宠物加载失败: ' + e);
  }
  return ok;
}

function renderBuddy(d) {
  const state = (d && d.state) || 'idle';
  const loc = (d && (typeof d.location === 'object' ? d.location : null)) || {};
  const locName = loc.name || d.location_name || (typeof d.location === 'string' ? d.location : '') || '—';
  const arriveAt = d && d.arrive_at ? (d.arrive_at < 1e12 ? d.arrive_at * 1000 : d.arrive_at) : 0;
  const dailyLimit = !!(d && d.daily_limit_reached);
  const durationH = (d && (d.duration_hours ?? d.duration_hour)) || 0;
  const rewardFor = d && d.reward_credit;
  let desc = '';
  if (state === 'traveling') desc = `正在前往「${locName}」，旅程约 ${durationH} 小时，预计 +${rewardFor ?? '?'} 积分。`;
  else if (state === 'arrived') desc = `已到达「${locName}」，可领取 ${rewardFor !== undefined ? rewardFor + ' 积分' : '奖励'}！`;
  else if (state === 'idle') desc = '宠物在家休息，选个地点派它出去旅行赚积分吧。';
  setBuddyState(state, locName, desc, arriveAt);
  $('buddy-loc').textContent = state === 'idle' ? '窝里休息 🏠' : locName;
  $('bd-today').textContent = dailyLimit ? '已派' : '未派';
  $('bd-credit').textContent = rewardFor !== undefined ? rewardFor : '—';
  $('bd-dur').textContent = durationH ? durationH + 'h' : '—';
  $('bd-next').textContent = dailyLimit ? '明日可派' : (rewardFor !== undefined ? rewardFor + ' 分' : '—');
  $('buddy-tip').textContent = dailyLimit ? '今日已派出，明日再来' : (state === 'idle' ? '可派出' : '');
  if (state === 'traveling' && arriveAt) {
    const c = $('buddy-countdown');
    c.dataset.at = arriveAt;
    c.style.display = 'flex';
    buddyCountdownTick();
  } else {
    $('buddy-countdown').style.display = 'none';
  }
  buddyLogAdd(`状态：${state}${locName !== '—' ? '（' + locName + '）' : ''}`, state === 'arrived' ? 'ok' : 'info');
}

function renderBuddyLocs() {
  const box = $('buddy-locs');
  if (!buddyLocations.length) { box.innerHTML = '<span class="empty">暂无可选地点</span>'; return; }
  box.innerHTML = '';
  buddyLocations.forEach(l => {
    const div = document.createElement('div');
    div.className = 'buddy-loc';
    div.title = '点击派出宠物到此地';
    const hTxt = l.hourMax && l.hourMax !== l.hour ? `${l.hour}-${l.hourMax}h` : (l.hour + 'h');
    const rTxt = l.rewardMax && l.rewardMax !== l.reward ? `${l.reward}-${l.rewardMax} 积分` : `+${l.reward} 积分`;
    div.innerHTML = `
      <div class="ln">📍 ${escapeHtml(l.name)}</div>
      <div class="lm">${escapeHtml(l.desc || `旅行 ${hTxt} 后返回`)}</div>
      <div class="lr">${rTxt} <small>· ${hTxt}</small></div>`;
    div.onclick = () => buddyDepart(l.id, l.name);
    box.appendChild(div);
  });
}

async function buddyDepart(locationId, name) {
  const btn = $('btn-buddy-depart');
  const id = String(locationId ?? (buddyLocations.length ? buddyLocations[0].id : ''));
  if (!id) { toast('无可派地点'); return; }
  const org = btn.textContent; btn.disabled = true; btn.textContent = '派出中…';
  try {
    const r = await invoke('buddy_depart', { locationId: id });
    const b = parseBody(r.body);
    if (r.error) { buddyLogAdd('派出失败: ' + r.error, 'err'); toast('派出失败: ' + r.error); }
    else if (r.status === 200) {
      buddyLogAdd(`已派出到「${name || id}」`, 'ok');
      toast(`宠物已出发前往「${name || id}」！`);
      loadBuddy();
    } else {
      const em = (b && (b.message || b.msg)) || ('HTTP ' + r.status);
      buddyLogAdd('派出失败(' + r.status + '): ' + em, 'err');
      toast('派出失败: ' + em);
    }
  } catch (e) { buddyLogAdd('派出出错: ' + e, 'err'); toast('派出出错: ' + e); }
  btn.disabled = false; btn.textContent = org;
}

async function buddyClaim() {
  const btn = $('btn-buddy-claim');
  btn.disabled = true;
  try {
    const r = await invoke('buddy_claim');
    const b = parseBody(r.body);
    if (r.error) { buddyLogAdd('领取失败: ' + r.error, 'err'); toast('领取失败: ' + r.error); }
    else if (r.status === 200) {
      const got = b && (b.credit || b.reward || b.amount);
      buddyLogAdd(`已领取奖励${got !== undefined ? '：+' + got + ' 积分' : ''}`, 'ok');
      toast('宠物奖励已领取 🎉');
      loadBuddy();
    }     else if (r.status === 400) {
      const em = (b && (b.msg || b.message)) || '暂无待领取的奖励';
      buddyLogAdd('领取未成功: ' + em, 'info');
      toast(em);
      loadBuddy();   // 刷新按钮/状态（避免 400 后按钮状态滞留）
    } else {
      const em = (b && (b.message || b.msg)) || ('HTTP ' + r.status);
      buddyLogAdd('领取失败(' + r.status + '): ' + em, 'err');
      toast('领取失败: ' + em);
    }
  } catch (e) { buddyLogAdd('领取出错: ' + e, 'err'); toast('领取出错: ' + e); loadBuddy(); }
  // 修复：失败/无奖励可领时不再永久禁用按钮（旧代码末尾恒 `disabled=true`，400/异常后按钮
  // 永远灰掉，必须手动刷新才恢复）。统一恢复后交由 setBuddyState 按 state 决定可用性。
  btn.disabled = false; btn.textContent = '🎁 领取奖励';
}

// ===== 多账号批量：签到 + 宠物（每个账号用各自 vault 快照登录态分别请求） =====

async function doCheckinAll() {
  toast('正在签到所有已登录账号…');
  const r = await invoke('checkin_all').catch(e => ({ error: String(e) }));
  if (r && r.error) { toast('批量签到失败: ' + r.error); return; }
  renderCheckinAll(r.results || []);
  const s = r.summary || {};
  toast(`批量签到完成：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`);
}

function renderCheckinAll(results) {
  const tb = $('ck-all-tbody'); if (!tb) return;
  if (!results.length) { tb.innerHTML = '<tr><td colspan="4" class="empty">无已登录账号</td></tr>'; return; }
  tb.innerHTML = results.map(r => {
    const nick = r.nickname ? privacy(r.nickname, { head: 3, tail: 4 }) : '';
    const uid = r.uid;
    const skipped = r.skipped;
    // 修复：无登录态快照等「未执行」情形后端标记 skipped=true（与宠物批量口径一致），
    // 渲染为「跳过」而非「失败」，并显示具体原因
    const state = r.error
      ? (r.skipped ? '<span class="pill" style="background:rgba(138,147,166,.18);color:var(--muted)">跳过</span>' : '<span class="soon">失败</span>')
      : (skipped ? '<span class="pill">已签到</span>'
      : (r.ok ? '<span class="ok">成功</span>' : '<span class="soon">未成功</span>'));
    const statusTxt = r.error ? (r.skipped ? '跳过' : '—') : (skipped ? '已签' : (r.ok ? '新签' : '—'));
    const msg = r.error ? escapeHtml(r.error) : (r.message || (skipped ? '今日已签到' : '签到成功'));
    return `<tr>
      <td><b>${escapeHtml(nick || uid)}</b><br><span style="font-size:10px;color:var(--muted)">${privacy(uid, { head: 4, tail: 4 })}</span></td>
      <td>${state}</td>
      <td>${statusTxt}</td>
      <td style="color:var(--muted)">${msg}</td>
    </tr>`;
  }).join('');
}

// ===== 全部账号额度（每个账号用各自 vault 快照登录态查官方 get-user-resource） =====
// 补齐 dashboard 只能看当前登录态单账号的局限：覆盖全部已登记账号，与本地消耗视图形成双视图。
async function loadQuotaAll() {
  try {
    const r = await invoke('quota_all').catch(e => ({ error: String(e) }));
    if (r && r.error) { toast('查询全部额度失败: ' + r.error); return; }
    renderQuotaAll(r.results || []);
    const s = r.summary || {};
    toast(`全部额度查询：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`);
  } catch (e) { toast('查询全部额度失败: ' + e); }
}

function renderQuotaAll(results, opts) {
  opts = opts || {};
  const tb = $('qa-tbody'); if (!tb) return;
  if (!results || !results.length) { tb.innerHTML = '<tr><td colspan="7" class="empty">无已登录账号</td></tr>'; return; }
  // 全量加载时记录原始结果，供搜索/排名过滤重渲染（不覆盖）
  if (opts.full !== false) lastQuotaAllResults = results;
  let rows = results.slice();
  // 排名模式：按全部剩余额度降序（仅影响展示顺序）
  if (opts.rank) {
    rows.sort((a, b) => {
      const qa = (a.parsed && Array.isArray(a.parsed.packages)) ? adaptParsed(a.parsed) : parseQuota(a.body);
      const qb = (b.parsed && Array.isArray(b.parsed.packages)) ? adaptParsed(b.parsed) : parseQuota(b.body);
      return ((qb && qb.grandRemain) || 0) - ((qa && qa.grandRemain) || 0);
    });
  }
  let sg = 0, st = 0, sq = 0, cnt = 0;
  const rankNo = !!opts.rank;
  tb.innerHTML = rows.map((r, idx) => {
    const nick = r.nickname ? privacy(r.nickname, { head: 3, tail: 4 }) : '';
    const uid = r.uid;
    const nameCell = (rankNo ? `<b style="color:var(--amber)">#${idx + 1}</b> ` : '') + `<b>${escapeHtml(nick || uid)}</b><br><span style="font-size:10px;color:var(--muted)">${privacy(uid, { head: 4, tail: 4 })}</span>`;
    if (r.error && !r.body) {
      const msg = r.skipped ? '无登录态快照' : escapeHtml(r.error);
      return `<tr><td>${nameCell}</td><td colspan="6" style="color:var(--muted)">${msg}</td></tr>`;
    }
    const q = (r.parsed && Array.isArray(r.parsed.packages)) ? adaptParsed(r.parsed) : parseQuota(r.body);
    if (!q) {
      return `<tr><td>${nameCell}</td><td colspan="6" style="color:var(--red)">解析失败 / 无数据</td></tr>`;
    }
    cnt++;
    sg += q.giftRemain; st += q.trialRemain; sq += q.grandRemain;
    const typ = r.status === 200 ? '<span class="ok">✓</span>' : (r.error ? '<span class="soon">失败</span>' : (r.skipped ? '跳过' : '—'));
    const exp = q.soonest ? qaExpChip(q.soonest.dl) : '长期';
    const ckBtn = `<button class="mini" onclick="qaCheckinFor('${uid}')">签到</button>`;
    return `<tr>
      <td>${nameCell}</td>
      <td>${typ}</td>
      <td class="num">${q.giftRemain.toFixed(2)}</td>
      <td class="num">${q.trialRemain.toFixed(2)}</td>
      <td class="num">${q.grandRemain.toFixed(2)}</td>
      <td>${exp}</td>
      <td style="white-space:nowrap;">${ckBtn}</td>
    </tr>`;
  }).join('');
  // 过滤（搜索）时保留全量汇总，避免数字随筛选跳变
  if (opts.full !== false) {
    $('qa-count').textContent = cnt;
    $('qa-gift').textContent = sg.toFixed(2);
    $('qa-trial').textContent = st.toFixed(2);
    $('qa-grand').textContent = sq.toFixed(2);
  }
}

// 到期预警配色：红 ≤7 天，橙 ≤30 天，绿 充裕
function qaExpChip(dl) {
  if (dl <= 0) return '<span class="soon">已过期</span>';
  if (dl <= 7) return `<span class="soon">${dl} 天</span>`;
  if (dl <= 30) return `<span style="color:var(--amber);font-weight:600;">${dl} 天</span>`;
  return `<span style="color:var(--green);">${dl} 天</span>`;
}

// ===== Batch C：趋势 / 预算 / 排名 / 搜索 / 星标 / 标签 / 导出（纯前端，localStorage 持久化） =====
let qaRankMode = false;
let lastQuotaAllResults = null;

// ---- 账号星标 / 标签（按 UID，localStorage） ----
const META_KEY = 'wbah_account_meta_v1';
function loadMetaMap() { try { return JSON.parse(localStorage.getItem(META_KEY) || '{}'); } catch { return {}; } }
function saveMetaMap(m) { try { localStorage.setItem(META_KEY, JSON.stringify(m)); } catch {} }
function getMeta(uid) { return Object.assign({ star: false, tags: [] }, loadMetaMap()[uid] || {}); }
function setMeta(uid, patch) { const m = loadMetaMap(); m[uid] = Object.assign({ star: false, tags: [] }, m[uid] || {}, patch); saveMetaMap(m); }
function toggleStar(uid) { const cur = getMeta(uid); setMeta(uid, { star: !cur.star }); if (accountsCache) showAccountList(accountsCache, (window.__login || {}).uid); return getMeta(uid).star; }
function setTagsFor(uid, tags) { const arr = Array.isArray(tags) ? tags : String(tags || '').split(',').map(s => s.trim()).filter(Boolean); setMeta(uid, { tags: arr }); if (accountsCache) showAccountList(accountsCache, (window.__login || {}).uid); }
function openTagInput(uid) {
  let v;
  try { v = (typeof window.prompt === 'function') ? window.prompt('设置标签（逗号分隔）：', getMeta(uid).tags.join(', ')) : null; }
  catch { v = null; }
  if (v === null || v === undefined) { if (v === null) toast('标签未改动'); return; }
  setTagsFor(uid, v); toast('标签已保存');
}

// ---- 额度趋势（本地按日追加，最多 90 点） ----
function trendKey(uid) { return 'wbah_trend_' + uid; }
function recordTrend(uid, remain) {
  if (!uid) return;
  const t = new Date(), ds = `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())}`;
  let arr = []; try { arr = JSON.parse(localStorage.getItem(trendKey(uid)) || '[]'); } catch {}
  if (!Array.isArray(arr)) arr = [];
  const last = arr[arr.length - 1];
  if (last && last.date === ds) last.remain = remain; else arr.push({ date: ds, ts: t.getTime(), remain });
  if (arr.length > 90) arr = arr.slice(-90);
  try { localStorage.setItem(trendKey(uid), JSON.stringify(arr)); } catch {}
}
function getTrend(uid) { try { return JSON.parse(localStorage.getItem(trendKey(uid)) || '[]'); } catch { return []; } }
function renderTrend(uid) {
  const svg = $('q-trend'); if (!svg) return;
  const arr = getTrend(uid);
  if (!arr.length) { svg.innerHTML = '<text x="160" y="58" fill="var(--muted)" font-size="11" text-anchor="middle">暂无趋势数据（打开/刷新额度后记录）</text>'; return; }
  const W = 320, H = 110, p = 8;
  const vals = arr.map(x => x.remain);
  const max = Math.max(1, ...vals), min = Math.min(0, ...vals), span = (max - min) || 1;
  const n = arr.length;
  const X = i => p + (n === 1 ? (W / 2 - p) : (i / (n - 1)) * (W - 2 * p));
  const Y = v => H - p - ((v - min) / span) * (H - 2 * p);
  const pts = arr.map((x, i) => `${X(i).toFixed(1)},${Y(x.remain).toFixed(1)}`).join(' ');
  const area = `${X(0).toFixed(1)},${(H - p).toFixed(1)} ${pts} ${X(n - 1).toFixed(1)},${(H - p).toFixed(1)}`;
  const lab = n > 1 ? `<text x="${X(0).toFixed(1)}" y="${H - 1}" fill="var(--muted)" font-size="8">${arr[0].date.slice(5)}</text><text x="${X(n - 1).toFixed(1)}" y="${H - 1}" fill="var(--muted)" font-size="8" text-anchor="end">${arr[n - 1].date.slice(5)}</text>` : '';
  svg.innerHTML =
    `<polygon points="${area}" fill="rgba(80,200,140,.12)"/>` +
    `<polyline points="${pts}" fill="none" stroke="var(--green)" stroke-width="1.6"/>` +
    arr.map((x, i) => `<circle cx="${X(i).toFixed(1)}" cy="${Y(x.remain).toFixed(1)}" r="1.8" fill="var(--green)"/>`).join('') +
    `<text x="${W - p}" y="${p + 8}" fill="var(--muted)" font-size="8" text-anchor="end">峰值 ${max.toFixed(1)}</text>` + lab;
}
function renderBudgetBar(q) {
  const fill = $('q-budget-fill'), txt = $('q-budget-txt');
  if (!fill || !txt || !q) return;
  const used = q.giftUsed || 0, size = q.giftSize || 0;
  const pct = size > 0 ? Math.min(100, Math.round(used / size * 100)) : 0;
  fill.style.width = pct + '%';
  txt.textContent = `赠送包已用 ${used.toFixed(2)} / ${size.toFixed(2)}（${pct}%）`;
}

// ---- 搜索 + 排名（基于最近一次 quota_all 结果重渲染） ----
function applyQuotaAllFilters() {
  if (!lastQuotaAllResults) return;
  const q = (($('qa-search') && $('qa-search').value) || '').trim().toLowerCase();
  let rows = lastQuotaAllResults.slice();
  if (q) rows = rows.filter(r => (r.nickname || '').toLowerCase().includes(q) || (r.uid || '').toLowerCase().includes(q));
  renderQuotaAll(rows, { full: false, rank: qaRankMode });
}
function qaToggleRank() { qaRankMode = !qaRankMode; applyQuotaAllFilters(); toast(qaRankMode ? '已按剩余额度排名' : '已关闭排名'); }

// ---- 导出脱敏 Markdown ----
function exportQuotaMd() {
  if (!lastQuotaAllResults) { toast('请先查询全部额度', 'err'); return; }
  const lines = ['# WorkBuddy 账户中枢 · 额度导出（脱敏）', '', `> 导出时间：${new Date().toLocaleString('zh-CN')}`, ''];
  for (const r of lastQuotaAllResults) {
    const uidm = r.uid ? privacy(r.uid, { head: 4, tail: 4, safe: false }) : '?';
    const nick = r.nickname ? privacy(r.nickname, { head: 3, tail: 4 }) : '?';
    if (r.error && !r.body) { lines.push(`- **${nick}** (\`${uidm}\`)：${r.skipped ? '无登录态快照' : (r.error || '失败')}`); continue; }
    const q = (r.parsed && Array.isArray(r.parsed.packages)) ? adaptParsed(r.parsed) : parseQuota(r.body);
    if (!q) { lines.push(`- **${nick}** (\`${uidm}\`)：解析失败 / 无数据`); continue; }
    lines.push(`- **${nick}** (\`${uidm}\`)：赠送包剩余 ${q.giftRemain.toFixed(2)} · 体验版 ${q.trialRemain.toFixed(2)} · 全部剩余 ${q.grandRemain.toFixed(2)} · 使用率 ${q.usePct}%`);
  }
  const blob = new Blob([lines.join('\n')], { type: 'text/markdown' });
  const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = `workbuddy-quota-${new Date().toISOString().slice(0, 10)}.md`; a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  toast('已导出脱敏额度 Markdown');
}

// 多账号到期时间轴已移除（v0.5.8）：到期预警改用额度表格内彩色 chip 表达。

async function qaCheckinFor(uid) {
  try {
    const r = await invoke('checkin_for', { uid });
    if (r && r.error) { toast('签到失败: ' + r.error, 'err'); return; }
    if (r.skipped) { toast('今日已签到'); }
    else if (r.status === 200) { toast('签到成功 ✅'); }
    else { toast('签到未完成（' + (r.status || '?') + '）'); }
    loadQuotaAll();   // 刷新该行：重查额度
  } catch (e) { toast('签到失败: ' + e, 'err'); }
}

// ===== 全部账号 AI 记忆画像（每个账号用各自 vault 快照登录态查 /api/memory/profile） =====
// 补齐单账号记忆面板只能看当前登录态的局限，覆盖全部已登记账号。
async function loadMemoryAll() {
  try {
    const r = await invoke('memory_all').catch(e => ({ error: String(e) }));
    if (r && r.error) { toast('查询全部记忆失败: ' + r.error, 'err'); return; }
    renderMemoryAll(r.results || []);
    const s = r.summary || {};
    toast(`全部记忆查询：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`);
  } catch (e) { toast('查询全部记忆失败: ' + e, 'err'); }
}

function renderMemoryAll(results) {
  const box = $('memory-all'); if (!box) return;
  if (!results || !results.length) { box.innerHTML = '<span class="empty">无已登录账号</span>'; return; }
  box.innerHTML = results.map(r => {
    const nick = r.nickname ? privacy(r.nickname, { head: 3, tail: 4 }) : '';
    const uid = r.uid;
    const head = `<div class="mem-head">🗂 <b>${escapeHtml(nick || uid)}</b> <span style="font-size:10px;color:var(--muted)">${privacy(uid, { head: 4, tail: 4 })}</span></div>`;
    if (r.error && !r.body) { const msg = r.skipped ? '无登录态快照' : escapeHtml(r.error); return `<div class="mem-card">${head}<div class="sec-body" style="margin-top:6px;color:var(--muted)">${msg}</div></div>`; }
    const d = r.body?.data;
    if (!d) { return `<div class="mem-card">${head}<div class="sec-body" style="margin-top:6px;color:var(--red)">无数据 / 解析失败</div></div>`; }
    const items = [
      ['用户 ID', privacy(d.user_id, { head: 4, tail: 4, safe: !d.user_id })],
      ['用户名', privacy(d.user_name, { head: 3, tail: 3, safe: !d.user_name })],
      ['更新时间', (d.updated_at || d.updatedAt || '—')], ['版本', d.version ?? '—'],
    ];
    const kv = items.map(([k, v]) => `<div class="item"><span class="k">${k}</span><span class="v">${v || '—'}</span></div>`).join('');
    const raw = d.foryou_prompt || d.memory || '';
    let sec = '';
    if (raw) {
      const parts = raw.split(/^##\s+/m).filter(Boolean);
      sec = parts.map(p => {
        const nl = p.indexOf('\n');
        const title = (nl > 0 ? p.slice(0, nl) : p).trim();
        const content = (nl > 0 ? p.slice(nl + 1) : '').trim();
        return `<details class="sec"><summary>${escapeHtml(title)}</summary><div class="sec-body">${escapeHtml(content)}</div></details>`;
      }).join('');
      if (!parts.length) sec = `<details class="sec"><summary>记忆内容</summary><div class="sec-body">${escapeHtml(raw)}</div></details>`;
    } else sec = '<span class="empty">无记忆画像</span>';
    const memo = raw ? `<span class="${keysRevealed ? 'privacy-text show' : 'privacy-text'}">${escapeHtml(raw)}</span>` : '(空)';
    return `<div class="mem-card">${head}<div class="kv" style="margin:8px 0;">${kv}</div><div class="memo" style="margin-bottom:6px;">${memo}</div>${sec}</div>`;
  }).join('');
}

async function refreshBuddyAll() {
  const r = await invoke('buddy_all_status').catch(e => ({ error: String(e) }));
  if (r && r.error) { buddyLogAdd('批量宠物状态失败: ' + r.error, 'err'); return; }
  renderBuddyAll(r.accounts || []);
}

function renderBuddyAll(accounts) {
  const tb = $('buddy-all-tbody'); if (!tb) return;
  if (!accounts.length) { tb.innerHTML = '<tr><td colspan="6" class="empty">无账号</td></tr>'; return; }
  tb.innerHTML = accounts.map(a => {
    const nick = a.nickname ? privacy(a.nickname, { head: 3, tail: 4 }) : '';
    const uid = a.uid;
    const noLogin = !a.has_login;
    const stateBadge = noLogin ? '<span class="soon">无登录态</span>'
      : (a.error ? '<span class="soon">错误</span>'
      : ({ idle: '<span class="pill">休息中</span>', traveling: '<span class="pill gift">旅行中</span>', arrived: '<span class="pill" style="background:rgba(39,192,138,.18);color:var(--green)">已到达</span>', unknown: '<span class="pill">未知</span>' }[a.state] || escapeHtml(a.state || '?')));
    const loc = a.location ? escapeHtml(String(a.location)) : '—';
    const reward = (a.reward_credit !== undefined && a.reward_credit !== null) ? (typeof a.reward_credit === 'object' ? JSON.stringify(a.reward_credit) : a.reward_credit) : '—';
    const arrive = a.arrive_at ? new Date((a.arrive_at < 1e12 ? a.arrive_at * 1000 : a.arrive_at)).toLocaleString() : '—';
    const idle = a.state === 'idle' && a.has_login && !a.error;
    const arrived = a.state === 'arrived' && a.has_login && !a.error;
    const departBtn = `<button class="mini" ${idle ? '' : 'disabled'} onclick="buddyDepartFor('${uid}')">派出</button>`;
    const claimBtn = `<button class="mini" ${arrived ? '' : 'disabled'} onclick="buddyClaimFor('${uid}')">领取</button>`;
    return `<tr>
      <td><b>${escapeHtml(nick || uid)}</b><br><span style="font-size:10px;color:var(--muted)">${privacy(uid, { head: 4, tail: 4 })}</span></td>
      <td>${stateBadge}</td>
      <td>${loc}</td>
      <td>${escapeHtml(String(reward))}</td>
      <td class="num" style="font-size:10px;">${arrive}</td>
      <td style="white-space:nowrap;">${departBtn} ${claimBtn}</td>
    </tr>`;
  }).join('');
}

// 批量派出/逐行派出的目标地点：与单账号「派出宠物」按钮一致，统一用宠物面板默认第一地点
function allLocId() {
  return buddyLocations.length ? buddyLocations[0].id : '';
}

async function buddyDepartAll() {
  const lid = allLocId();
  if (!lid) { toast('请先选择派出地点'); return; }
  const btn = $('btn-buddy-depart-all'); if (btn) btn.disabled = true;
  buddyLogAdd('开始一键派出全部账号宠物 → ' + lid, 'info');
  const r = await invoke('buddy_all_depart', { locationId: String(lid) }).catch(e => ({ error: String(e) }));
  if (btn) btn.disabled = false;
  if (r && r.error) { toast('批量派出失败: ' + r.error); buddyLogAdd('批量派出失败: ' + r.error, 'err'); return; }
  // 先把逐账号结果渲染到表格（含成功/跳过/失败与原因），稍后 refreshBuddyAll 刷新为实时状态
  renderBuddyDepartAll(r.results || []);
  const s = r.summary || {};
  toast(`派出完成：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`);
  buddyLogAdd(`一键派出全部：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`, 'ok');
  setTimeout(refreshBuddyAll, 1800);
}

// 渲染「一键派出全部」的逐账号结果（三级：成功 / 跳过 / 失败，并说明跳过原因）
function renderBuddyDepartAll(results) {
  const tb = $('buddy-all-tbody'); if (!tb) return;
  if (!results.length) { tb.innerHTML = '<tr><td colspan="6" class="empty">无已登录账号</td></tr>'; return; }
  tb.innerHTML = results.map(r => {
    const nick = r.nickname ? privacy(r.nickname, { head: 3, tail: 4 }) : '';
    const uid = r.uid;
    let outcome, reason;
    if (r.error) { outcome = '<span class="soon">请求失败</span>'; reason = r.error; }
    else if (r.skipped) { outcome = '<span class="pill" style="background:rgba(138,147,166,.18);color:var(--muted)">跳过</span>'; reason = r.reason || r.message || '—'; }
    else if (r.ok) { outcome = '<span class="ok">派出成功</span>'; reason = r.reason || r.message || '已出发'; }
    else { outcome = '<span class="soon">失败</span>'; reason = r.reason || r.message || ('HTTP ' + (r.status || '?')); }
    return `<tr>
      <td><b>${escapeHtml(nick || uid)}</b><br><span style="font-size:10px;color:var(--muted)">${privacy(uid, { head: 4, tail: 4 })}</span></td>
      <td>${outcome}</td>
      <td colspan="3" style="color:var(--muted)">${escapeHtml(String(reason))}</td>
      <td style="white-space:nowrap;">—</td>
    </tr>`;
  }).join('');
}

async function buddyClaimAll() {
  const btn = $('btn-buddy-claim-all'); if (btn) btn.disabled = true;
  buddyLogAdd('开始一键领取全部账号奖励', 'info');
  const r = await invoke('buddy_all_claim').catch(e => ({ error: String(e) }));
  if (btn) btn.disabled = false;
  if (r && r.error) { toast('批量领取失败: ' + r.error); buddyLogAdd('批量领取失败: ' + r.error, 'err'); return; }
  renderBuddyDepartAll(r.results || []);
  const s = r.summary || {};
  toast(`领取完成：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`);
  buddyLogAdd(`一键领取全部：成功 ${s.ok || 0} · 跳过 ${s.skipped || 0} · 失败 ${s.fail || 0}`, 'ok');
  setTimeout(refreshBuddyAll, 1800);
}

async function buddyDepartFor(uid) {
  const lid = allLocId();
  if (!lid) { toast('请先选择派出地点'); return; }
  buddyLogAdd('派出账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' → ' + lid, 'info');
  const r = await invoke('buddy_depart_for', { uid: uid, locationId: String(lid) }).catch(e => ({ error: String(e) }));
  if (r && r.error) { toast('派出失败: ' + r.error); buddyLogAdd('派出失败: ' + r.error, 'err'); return; }
  if (r.skipped) {
    toast('已跳过: ' + (r.reason || '无法派出'));
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 跳过: ' + (r.reason || ''), 'info');
  } else if (r.ok) {
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 派出成功', 'ok');
  } else {
    const em = r.reason || r.message || JSON.stringify(r.body || '');
    toast('派出失败: ' + em);
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 派出失败: ' + String(em).slice(0, 80), 'err');
  }
  setTimeout(refreshBuddyAll, 1200);
}

async function buddyClaimFor(uid) {
  buddyLogAdd('领取账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 奖励', 'info');
  const r = await invoke('buddy_claim_for', { uid: uid }).catch(e => ({ error: String(e) }));
  if (r && r.error) { toast('领取失败: ' + r.error); buddyLogAdd('领取失败: ' + r.error, 'err'); return; }
  if (r.skipped) {
    toast('无需领取: ' + (r.reason || '无待领取奖励'));
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 跳过领取: ' + (r.reason || ''), 'info');
  } else if (r.ok) {
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 领取成功', 'ok');
  } else {
    const em = r.reason || r.message || JSON.stringify(r.body || '');
    toast('领取失败: ' + em);
    buddyLogAdd('账号 ' + privacy(uid, { head: 4, tail: 4 }) + ' 领取失败: ' + String(em).slice(0, 80), 'err');
  }
  setTimeout(refreshBuddyAll, 1200);
}

// 自动轮询：旅行中每 20s 同步状态，到达后自动领取
function startBuddyPoll() {
  if (buddyTimer) clearInterval(buddyTimer);
  buddyTimer = setInterval(async () => {
    try {
      const r = await invoke('buddy_status');
      if (r && r.error) return;
      if (r && r.status === 200) {
        const b = parseBody(r.body);
        const d = buddyData(b);
        if (d && d.state === 'arrived') {
          buddyLogAdd('检测到宠物已到达，自动领取奖励…', 'ok');
          await buddyClaim();
        } else if (d && d.arrive_at) {
          const at = d.arrive_at < 1e12 ? d.arrive_at * 1000 : d.arrive_at;
          if (at <= Date.now() && d.state === 'traveling') {
            buddyLogAdd('旅程时间到，刷新状态…', 'info');
            loadBuddy();
          }
        }
      }
    } catch (e) { /* 静默，避免频繁打扰 */ }
  }, 20000);
}

// ===== 数据加载 =====
async function loadAll() {
  try {
    bootLog('启动加载：开始…');
    const ro = $('raw-out'); if (ro) ro.textContent = '请求中…';

    // 登录态默认自动保存（防数据丢失）
    try { ensureSnapshot(); bootLog('启动加载：快照已确认'); }
    catch (e) { bootLog('启动加载：快照忽略 ' + e.message, 'err'); }

    // get_all 现在只返回本地信息（昵称/UID/账号列表/环境/JWT），瞬时返回，
    // 不再被网络请求卡住 —— 这是「进软件加载不出昵称」的根因修复点。
    bootLog('启动加载：请求 get_all…');
    const j = await invokeWithTimeout('get_all', {}, 5000);
    bootLog('启动加载：get_all 返回 ' + (j && typeof j), 'ok');
    if (ro) ro.textContent = JSON.stringify(j, null, 2);

    window.__login = j.login || {};
    // #27：依据 get_all 带回的 workbuddy_running 立即刷新顶部提示条，并启动周期轮询
    try { updateClientStatus(j.workbuddy_running); } catch (e) {}
    if (!window.__clientPoll) { window.__clientPoll = setInterval(pollClientStatus, 60000); }
    try { renderSidebar(j); bootLog('启动加载：侧边栏已渲染'); }
    catch (e) { bootLog('启动加载：侧边栏渲染失败 ' + e.message, 'err'); }

    try { renderAccount(j.login); bootLog('启动加载：账户信息已渲染'); }
    catch (e) { bootLog('启动加载：账户信息渲染失败 ' + e.message, 'err'); }

    try { renderJwt(j.jwt); bootLog('启动加载：JWT 已渲染'); }
    catch (e) { bootLog('启动加载：JWT 渲染失败 ' + e.message, 'err'); }

    try { renderEnv(j.env); bootLog('启动加载：环境已渲染'); }
    catch (e) { bootLog('启动加载：环境渲染失败 ' + e.message, 'err'); }

    $('updated').textContent = ' · 本地已加载 ' + new Date().toLocaleTimeString();
    refreshAccounts();
    loadModels();
    // 网络部分（额度/签到/记忆）独立分批加载，互不阻塞，失败不影响本地信息
    loadNetworkParts();
  } catch (e) {
    bootLog('启动加载失败：' + e, 'err');
    const ro = $('raw-out'); if (ro) ro.textContent = '错误：' + e;
    throw e; // 让 bootBootstrap 的重试机制接管
  }
}

// 额度/签到/记忆分开独立拉取（后端各自带 15s 超时），
// 任一部分慢/失败都不会卡住其余，更不会卡住本地昵称。
// 启动初期网络/后端可能未就绪，失败时自动重试（最多 4 次，间隔递增），
// 避免「打开软件后额度/签到/记忆空白，要点刷新全部才出来」。
let checkinInFlight = false;
// 自动签到（v0.5.10）：进入软件即触发一次 + 每 3h 轮询（对标 WorkDaddy daemon.js:6669 每3h setInterval + 开面板触发）
// 防重入对标 daemon.js:2218 claimInFlight；后端 do_checkin_as 已幂等（已签返回 skipped）
function autoCheckin() {
  if (checkinInFlight) return;
  checkinInFlight = true;
  invoke('checkin_all').then(r => {
    if (r && r.summary) console.info('auto checkin:', r.summary);
  }).catch(e => { console.warn('auto checkin err', e); })
    .finally(() => { checkinInFlight = false; });
}
function loadNetworkParts() {
  const retry = (fn, tries) => {
    fn().then(ok => {
      if (!ok && tries < 4) setTimeout(() => retry(fn, tries + 1), 800 * (tries + 1));
    });
  };
  retry(() => invoke('get_quota').then(j => {
    if (j && j.status === 200) { renderQuota(j); return true; }
    return false;   // 静默失败，交给重试兜底，避免启动时反复弹 toast
  }).catch(e => { console.warn('quota err', e); return false; }), 0);

  retry(() => invoke('get_checkin').then(j => {
    if (j && j.status === 200) { window.__checkin = j.body || {}; renderCheckin(parseBody(j.body)); return true; }
    return false;
  }).catch(e => { console.warn('checkin err', e); return false; }), 0);

  // 全部账号额度：覆盖 dashboard 单账号局限，启动时自动拉取（失败重试）
  retry(() => invoke('quota_all').then(j => {
    if (j && j.ok && (j.results || []).length) { renderQuotaAll(j.results); return true; }
    return false;
  }).catch(e => { console.warn('quota_all err', e); return false; }), 0);

  retry(() => invoke('get_memory').then(j => {
    if (j && j.status === 200) { renderMemory(parseBody(j.body)); return true; }
    return false;
  }).catch(e => { console.warn('memory err', e); return false; }), 0);

  // 用量与对话历史：进入软件默认获取（无需手动点刷新）
  retry(() => {
    const cur = (window.__login && window.__login.uid) || '';
    if (!cur) return false;
    return uhRefresh(cur, true).then(() => true).catch(() => false);
  }, 0);

  // 自动签到已按用户要求撤销（v0.5.16）：不再启动即触发、也不再每 3h 轮询系统级定时。
  // 仅保留手动「一键签到」按钮（调用 checkin_all）。这同时避免了后台无 GUI 时仍保活/轮询的行为。
}
// #27：主客户端（WorkBuddy）进程存在性提示（复用 app_running 命令）；
// 关闭主客户端后签到 / 切换账号会静默失败，顶部给出明确提示而非静默报错。
function updateClientStatus(running) {
  const bar = document.getElementById('client-status-bar');
  if (!bar) return;
  if (running) {
    bar.className = 'client-bar ok hidden';
    bar.innerHTML = '';
  } else {
    bar.className = 'client-bar warn';
    bar.innerHTML = '<span class="dot"></span><span>未检测到 WorkBuddy 主客户端在运行 —— 签到 / 切换账号可能失败，请先打开 WorkBuddy 再操作。</span>';
  }
}
function pollClientStatus() {
  invoke('app_running').then(r => updateClientStatus(!!r)).catch(() => {});
}

async function loadQuota() {
  $('raw-out').textContent = '请求中…';
  try {
    const j = await invoke('get_quota');
    $('raw-out').textContent = JSON.stringify(j, null, 2);
    if (j.status === 200) renderQuota(j);
  } catch (e) { $('raw-out').textContent = '错误：' + e; }
}
async function doCheckin() {
  $('raw-out').textContent = '签到中…';
  try {
    const j = await invoke('do_checkin');
    $('raw-out').textContent = JSON.stringify(j, null, 2);
    toast(j.skipped ? '今日已签到' : (j.error ? '签到失败: ' + j.error : '签到完成'));
    loadAll();
  } catch (e) { $('raw-out').textContent = '错误：' + e; }
}

// ===== API 管理（自定义 + 官方 + 当前使用） =====

let editingId = null;          // null=新增；否则为编辑中的模型 id
let customModelsCache = [];    // 掩码列表（展示用）
let customResp = {};           // 自定义 API 原始响应（含 ok/error/path）
let officialCache = [];        // 官方模型清单
let currentCache = null;       // 当前使用模型探测结果
let keysRevealed = false;      // 全局眼睛：true=显示所有 Key 中段

async function loadModels() {
  let custom, official, current;
  try { custom = await invoke('list_custom_models'); }
  catch (e) { custom = { ok: false, error: String(e) }; }
  try { official = await invoke('official_models'); }
  catch (e) { official = []; }
  try { current = await invoke('current_model'); }
  catch (e) { current = null; }
  customResp = (custom && typeof custom === 'object') ? custom : { ok: false, error: '返回格式异常' };
  customModelsCache = (customResp.models && Array.isArray(customResp.models)) ? customResp.models : [];
  officialCache = Array.isArray(official) ? official : [];
  currentCache = current;
  renderApiPanel();
}

function renderApiPanel() {
  // 路径
  $('api-path').textContent = (customResp.path || '~/.workbuddy/models.json');
  // 当前使用
  const cur = currentCache && currentCache.ok ? currentCache.model_id : null;
  if (cur) {
    $('api-current').innerHTML = `<span class="dot"></span> 当前使用：<b>${escapeHtml(cur)}</b><span style="color:var(--muted);font-size:11px;">（探测自本机 WorkBuddy 状态 · 官方渠道）</span>`;
  } else {
    $('api-current').innerHTML = `<span style="color:var(--muted);">当前使用：未能探测（打开过 WorkBuddy 后刷新）</span>`;
  }
  // 自定义列表
  const box = $('api-list');
  if (customResp.ok === false) {
    box.innerHTML = `<span class="empty" style="color:var(--red);">${escapeHtml(customResp.error || '读取失败')}</span>`;
    return;
  }
  if (!customModelsCache.length) { box.innerHTML = '<span class="empty">暂无自定义模型，点「+ 新增」添加</span>'; return; }
  box.innerHTML = '';
  customModelsCache.forEach(m => {
    const initial = (m.name || m.id || '?').slice(0, 2);
    const sid = escId(m.id);
    // Key 属于隐私数据，统一走 privacy()（中段高斯模糊，全局眼睛控制）
    let keyHtml;
    if (m._hasKey) {
      const fullKey = m.apiKey || '****';
      keyHtml = `<span class="key-sec">${privacy(fullKey, { head: 6, tail: 4 })}</span>`;
    } else {
      keyHtml = `<span class="key-sec"><span class="psec"><span class="pre">无 Key</span></span></span>`;
    }
    const row = document.createElement('div');
    row.className = 'api-row';
    row.innerHTML = `
      <div class="av">${escapeHtml(initial)}</div>
      <div class="info">
        <div class="n">${escapeHtml(m.name || m.id)}${cur === m.id ? '<span class="badge cur">当前</span>' : ''}<span class="badge">${escapeHtml(m.vendor || m.provider || 'Custom')}</span></div>
        <div class="s">${escapeHtml(m.id)} · ${privacy(m.url, { head: 8, tail: 4, safe: !m.url })} · ${keyHtml} · ctx ${m.contextWindow || m.maxInputTokens || '?'}</div>
        <div class="tst" id="tst-${sid}"></div>
      </div>
      <div class="ops">
        <button class="mini secondary" onclick="testModel('${sid}')">测试</button>
        <button class="mini secondary" onclick="editModel('${sid}')">编辑</button>
        <button class="mini" style="background:var(--red);" onclick="delModel('${sid}')">删除</button>
      </div>`;
    box.appendChild(row);
  });
  // 官方列表
  const ob = $('official-list');
  if (!officialCache.length) { ob.innerHTML = '<span class="empty">加载失败或无内置模型</span>'; return; }
  ob.innerHTML = '';
  officialCache.forEach(m => {
    const row = document.createElement('div');
    row.className = 'api-row';
    const isCur = cur === m.id;
    row.innerHTML = `
      <div class="av" style="background:rgba(43,108,255,.15);color:#7ea6ff;">官</div>
      <div class="info">
        <div class="n">${escapeHtml(m.name)}${isCur ? '<span class="badge cur">当前使用</span>' : ''}<span class="badge builtin">官方内置</span></div>
        <div class="s">${escapeHtml(m.id)} · ${escapeHtml(m.vendor || '')}</div>
      </div>
      <div class="ops"><span class="badge">不可编辑</span></div>`;
    ob.appendChild(row);
  });
}

function escId(id) { return String(id).replace(/[^a-zA-Z0-9_-]/g, '_'); }

// 把 key 拆成 前缀(pre)+中段(mid)+后缀(suf)，仅中段做高斯模糊
// 全局小眼睛：一次性切换所有隐私数据（Key/UID/Token 等）中段的 模糊/清晰
function toggleAllKeys() {
  keysRevealed = !keysRevealed;
  const btn = $('eye-global');
  if (btn) {
    btn.classList.toggle('off', !keysRevealed);
    btn.title = keysRevealed ? '点击恢复隐私模糊' : '显示所有隐私数据中段（Key/UID/Token 等）';
    btn.innerHTML = keysRevealed ? '🙈 隐私' : '👁 隐私';
  }
  // 统一控制所有隐私分段中段
  document.querySelectorAll('.psec .mid').forEach(m => {
    m.classList.toggle('show', keysRevealed);
  });
  // 整段隐私文本（记忆画像等）：默认模糊，眼睛打开时清晰
  document.querySelectorAll('.privacy-text').forEach(m => {
    m.classList.toggle('show', keysRevealed);
  });
  // API 区 key 也是隐私分段，重渲染刷新其内容（或由 DOM 切换覆盖，这里兜底）
  if (typeof renderApiPanel === 'function') renderApiPanel();
}

function openModelModal(title) {
  $('model-modal-title').textContent = title;
  $('mf-err').style.display = 'none';
  $('model-modal').classList.add('show');
}
function closeModelModal() { $('model-modal').classList.remove('show'); editingId = null; }

function addModel() {
  editingId = null;
  ['mf-name', 'mf-id', 'mf-url', 'mf-key', 'mf-provider', 'mf-label'].forEach(i => $(i).value = '');
  $('mf-id').disabled = false;
  $('mf-ctx').value = '128000'; $('mf-max').value = '8192';
  $('mf-tool').checked = true; $('mf-img').checked = false; $('mf-reason').checked = false;
  openModelModal('新增自定义 API');
}
function editModel(id) {
  const m = customModelsCache.find(x => x.id === id);
  if (!m) return toast('未找到模型');
  editingId = id;
  $('mf-name').value = m.name || '';
  $('mf-id').value = m.id || '';
  $('mf-id').disabled = true;
  $('mf-url').value = m.url || '';
  $('mf-key').value = '';                 // 留空 = 保持原 key
  $('mf-provider').value = m.provider || m.vendor || '';
  $('mf-label').value = m.label || '';
  $('mf-ctx').value = m.contextWindow || m.maxInputTokens || 128000;
  $('mf-max').value = m.maxTokens || m.maxOutputTokens || 8192;
  $('mf-tool').checked = m.supportsToolCall !== false;
  $('mf-img').checked = m.supportsImages === true;
  $('mf-reason').checked = m.supportsReasoning === true;
  openModelModal('编辑：' + m.name);
}
function delModel(id) {
  const m = customModelsCache.find(x => x.id === id);
  if (!confirm('确认删除自定义 API「' + (m ? m.name : id) + '」？\n（WorkBuddy 重启后该模型将不可用）')) return;
  invoke('delete_custom_model', { id }).then(r => {
    toast(r.ok ? '已删除' : '删除失败');
    loadModels();
  }).catch(e => toast('删除失败: ' + e));
}
function testModel(id) {
  const m = customModelsCache.find(x => x.id === id);
  if (!m) return;
  const el = $('tst-' + escId(id));
  if (el) el.textContent = '测试中…';
  invoke('test_custom_model', { model: m }).then(r => {
    if (el) el.innerHTML = r.ok
      ? `<span class="ok">✓ ${r.status} · ${r.ms}ms · ${r.model ? escapeHtml(String(r.model)) : ''}</span>`
      : `<span class="err">✗ ${escapeHtml(r.error || ('HTTP ' + r.status))}</span>`;
  }).catch(e => { if (el) el.innerHTML = `<span class="err">✗ ${escapeHtml(e)}</span>`; });
}
function testCurrentForm() {
  const err = $('mf-err');
  err.style.display = 'none';
  const name = $('mf-name').value.trim(), id = $('mf-id').value.trim(),
    url = $('mf-url').value.trim(), key = $('mf-key').value.trim();
  if (!id || !url) { err.textContent = '请填写 模型 ID 和 API 地址'; err.style.display = 'block'; return; }
  err.textContent = '测试中…'; err.style.display = 'block';
  const m = { id, name, url, apiKey: key };
  invoke('test_custom_model', { model: m }).then(r => {
    err.textContent = r.ok ? '✓ 连接成功 (' + r.ms + 'ms' + (r.model ? ' · ' + r.model : '') + ')' : '✗ ' + (r.error || ('HTTP ' + r.status));
    err.style.color = r.ok ? 'var(--green)' : 'var(--red)';
  }).catch(e => { err.textContent = '✗ ' + e; err.style.color = 'var(--red)'; });
}
async function saveModel() {
  const err = $('mf-err');
  err.style.display = 'none';
  const name = $('mf-name').value.trim(), id = $('mf-id').value.trim(),
    url = $('mf-url').value.trim(), key = $('mf-key').value.trim(),
    provider = $('mf-provider').value.trim(), label = $('mf-label').value.trim(),
    ctx = parseInt($('mf-ctx').value) || 128000, max = parseInt($('mf-max').value) || 8192;
  if (!id || !url) { err.textContent = '请填写 模型 ID 和 API 地址'; err.style.display = 'block'; return; }
  // 修复：新增时必须填 API Key（否则会存出永远测不通的模型）；编辑留空 = 保持原 key
  if (!editingId && !key) { err.textContent = '新增模型必须填写 API Key'; err.style.display = 'block'; return; }
  const payload = {
    name: name || id, id,
    url, apiKey: key,
    supportsToolCall: $('mf-tool').checked,
    supportsImages: $('mf-img').checked,
    supportsReasoning: $('mf-reason').checked,
  };
  if (provider) payload.provider = provider;
  if (label) payload.label = label;
  payload.contextWindow = ctx;
  payload.maxTokens = max;
  try {
    if (editingId) {
      await invoke('update_custom_model', { id: editingId, patch: payload });
      toast('已保存');
    } else {
      await invoke('add_custom_model', { model: payload });
      toast('已新增');
    }
    closeModelModal();
    loadModels();
  } catch (e) { err.textContent = String(e); err.style.display = 'block'; }
}
async function restartWB() {
  if (!confirm('重启 WorkBuddy 使模型配置生效？（会关闭当前 WorkBuddy 窗口再启动）')) return;
  try { await invoke('restart_workbuddy'); toast('已发送重启 WorkBuddy'); }
  catch (e) { toast('重启失败: ' + e); }
}

// 暴露给 inline onclick（安全：未定义的函数跳过，不影响其余，更不阻断 bootBootstrap）
(function expose(){
  const names = ['addModel','editModel','delModel','testModel','testCurrentForm','saveModel','closeModelModal','restartWB','loadAll','loadQuota','doCheckin','openReport','closeReport','copyReport','ensureSnapshot','backupAll','saveCurrentLogin','confirmSwitchYes','confirmSwitchNo','switchTo','openBackups','renderBackups','openBackupDetail','closeBackups','closeBackupDetail','loadBuddy','buddyDepart','buddyClaim','buddyDepartAll','buddyClaimAll','buddyDepartFor','buddyClaimFor','refreshBuddyAll','doCheckinAll','loadQuotaAll','qaCheckinFor','toggleAllKeys','refreshAll','uhRefresh','uhExportJson','uhExportCsv','loadMemoryAll','qaToggleRank','exportQuotaMd','applyQuotaAllFilters','toggleStar','openTagInput'];
  for (const n of names) {
    try {
      const fn = eval(n);
      if (typeof fn === 'function') window[n] = fn;
    } catch (e) { console.warn('[expose] skip', n, e && e.message); }
  }
  window.snapshotCurrent = ensureSnapshot; // 兼容旧内联引用（函数已重构为 ensureSnapshot）
})();

// 一键「刷新全部」：本地信息 + 网络部分 + 宠物面板一次性全部刷新（含宠物，避免只点宠物按钮才出）
function refreshAll() { loadAll(); loadBuddy(); }

// ===== 用量与对话历史面板 =====
async function uhCurrentUid() {
  try { const r = await invoke('list_accounts'); return (r && r.current_uid) ? r.current_uid : ''; }
  catch { return ''; }
}
function uhFmtTs(ms) {
  if (!ms || ms <= 0) return '—';
  const d = new Date(Number(ms));
  if (isNaN(d.getTime())) return String(ms);
  const p = n => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
function uhFmtNum(n) {
  try { return Number(n).toLocaleString('en-US'); } catch { return String(n); }
}
async function uhRefresh(forceUid, silent) {
  let uid = forceUid || $('uh-uid').value.trim();
  if (!uid) uid = await uhCurrentUid();
  if (!uid) { if (!silent) toast('无法获取当前账号 UID', 'err'); return; }
  const limit = Math.max(1, Math.min(500, parseInt($('uh-limit').value || '50', 10) || 50));
  try {
    const r = await invoke('usage_summary', { uid, limit });
    const data = JSON.parse(r);
    const rows = data.conversations || [];
    const tb = $('uh-rows');
    tb.innerHTML = '';
    if (!rows.length) {
      tb.innerHTML = '<tr><td colspan="5" style="padding:10px;color:var(--muted);">该账号暂无对话消耗记录</td></tr>';
    } else {
      for (const c of rows) {
        const tr = document.createElement('tr');
        tr.style.borderBottom = '1px solid var(--line)';
        const title = (c.title || '(无标题)').replace(/[<>&]/g, '');
        tr.innerHTML = `<td style="padding:6px 8px;">${title}</td>`
          + `<td style="padding:6px 8px;color:var(--muted);">${c.model || '—'}</td>`
          + `<td style="padding:6px 8px;text-align:right;">${uhFmtNum(c.used || 0)}</td>`
          + `<td style="padding:6px 8px;text-align:right;">${(c.credits || 0).toFixed(2)}</td>`
          + `<td style="padding:6px 8px;color:var(--muted);">${uhFmtTs(c.last_activity_at)}</td>`;
        tb.appendChild(tr);
      }
    }
    $('uh-totals').textContent = `共 ${data.count} 条 · 总消耗 token ${uhFmtNum(data.total_used || 0)} · 总费用 ${(data.total_credits || 0).toFixed(2)} 积分 · 账号 ${uid}`;
  } catch (e) { if (!silent) toast('刷新用量失败: ' + e, 'err'); }
}
async function uhExportJson() {
  let uid = $('uh-uid').value.trim();
  if (!uid) uid = await uhCurrentUid();
  if (!uid) { toast('无法获取当前账号 UID', 'err'); return; }
  try {
    const r = await invoke('export_conversation_history', { uid, include_deleted: false, with_usage: true });
    const blob = new Blob([r], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'conversation_history_' + uid + '.json';
    a.click();
    URL.revokeObjectURL(a.href);
    toast('已导出对话历史（JSON 已下载，含消耗）', 'ok');
  } catch (e) { toast('导出失败: ' + e, 'err'); }
}
async function uhExportCsv() {
  let uid = $('uh-uid').value.trim();
  if (!uid) uid = await uhCurrentUid();
  if (!uid) { toast('无法获取当前账号 UID', 'err'); return; }
  try {
    const r = await invoke('export_conversation_history', { uid, include_deleted: false, with_usage: true });
    const data = JSON.parse(r);
    const rows = data.conversations || [];
    const head = ['id', 'title', 'model', 'status', 'used', 'size', 'credits', 'created_at', 'updated_at', 'last_activity_at', 'deleted_at'];
    const esc = v => { const s = v == null ? '' : String(v); return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s; };
    const lines = [head.join(',')];
    for (const c of rows) lines.push(head.map(k => esc(c[k])).join(','));
    const blob = new Blob(['\ufeff' + lines.join('\n')], { type: 'text/csv;charset=utf-8' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'conversation_history_' + uid + '.csv';
    a.click();
    URL.revokeObjectURL(a.href);
    toast('已导出对话历史（CSV 已下载）', 'ok');
  } catch (e) { toast('导出 CSV 失败: ' + e, 'err'); }
}

// ==== 启动时序优化 ====
// 问题：webview 首次初始化时 invoke 可能偶发失败 / 后端刚就绪，导致
// 本地信息、网络部分（额度/签到/记忆）、宠物面板不自动显示，要点「刷新全部」或再点宠物才出来。
// 方案：分阶段自动加载 + 失败自动重试，保证用户打开即见全部数据，无需手动操作。
function bootBootstrap() {
  // 初始化右上角消息徽标
  try { NotifyCenter.updateBadge(); } catch (e) {}
  // 真正判断 Tauri IPC 是否就绪：window.__TAURI__.core.invoke 必须存在
  function ready() {
    try { return !!(window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function'); }
    catch { return false; }
  }
  async function tryLoadAll(retries = 0) {
    if (!ready()) {
      bootLog('等待 Tauri IPC 注入… (' + retries + ')');
      if (retries < 40) { setTimeout(() => tryLoadAll(retries + 1), 250); return; }
      bootLog('Tauri IPC 未注入，停止自动加载', 'err');
      return;
    }
    bootLog('Tauri IPC 已就绪，开始加载…', 'ok');
    try {
      await loadAll();
      bootLog('启动加载完成', 'ok');
      // 诊断条常驻会遮挡 UI，加载成功后 4 秒自动隐藏（失败时保留便于排查）
      setTimeout(() => { const el = $('boot-log'); if (el) el.classList.remove('show'); }, 4000);
    } catch (e) {
      bootLog('启动加载失败，5秒后重试：' + e.message, 'err');
      if (retries < 8) setTimeout(() => tryLoadAll(retries + 1), 5000);
    }
  }
  async function tryBuddy(retries = 0) {
    if (!ready()) { if (retries < 40) setTimeout(() => tryBuddy(retries + 1), 250); return; }
    try {
      const ok = await loadBuddy();
      if (!ok && retries < 6) setTimeout(() => tryBuddy(retries + 1), 2000);
      else if (ok) bootLog('宠物面板加载完成', 'ok');
    } catch (e) {
      if (retries < 6) setTimeout(() => tryBuddy(retries + 1), 2000);
    }
  }
  tryLoadAll();
  setTimeout(() => tryBuddy(), 800);
  setTimeout(() => {
    if (ready()) startBuddyPoll(); // 自动轮询 + 到达自动领奖
  }, 3000);
}
bootLog('脚本末尾：准备调用 bootBootstrap');
bootBootstrap();
bootLog('脚本末尾：bootBootstrap 已调用');
