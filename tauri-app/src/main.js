// WorkBuddy 账户中枢 — Tauri 前端（调用 Rust Command 替代 fetch）
// 仪表盘视觉沿用 web 版本；网络/账号操作全部走 invoke。
const invoke = (cmd, args = {}) => window.__TAURI__.core.invoke(cmd, args);

function $(id) { return document.getElementById(id); }
function toast(msg) {
  const t = $('toast');
  if (!t) return;
  t.textContent = msg; t.classList.add('show');
  setTimeout(() => t.classList.remove('show'), 2400);
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
  if (!regs.length) { al.innerHTML = '<div class="empty">无已登记账号</div>'; }
  regs.forEach(a => {
    const div = document.createElement('div');
    div.className = 'acc' + (a.uid === cur ? ' active' : '');
    const initial = (a.nickname || a.uid || '?').slice(0, 2);
    const isCur = a.uid === cur;
    const snapped = a.has_snapshot;
    const badge = isCur
      ? '<span class="cur">当前</span>'
      : (snapped
        ? `<button class="mini" onclick="event.stopPropagation();switchTo('${a.uid}')">切换</button>`
        : '<span class="mini" style="background:var(--line);color:var(--muted);padding:3px 9px;border-radius:6px;font-size:10.5px;">未快照</span>');
    const snapBtn = isCur ? '<button class="mini" onclick="event.stopPropagation();snapshotCurrent()">快照当前</button>' : '';
    div.innerHTML = `
      <div class="dot">${initial}</div>
      <div class="info"><div class="n">${a.nickname ? privacy(a.nickname, { head: 3, tail: 4 }) : '(无昵称)'}</div><div class="s">${privacy(a.uid, { head: 4, tail: 4 })}</div></div>
      ${badge}${snapBtn}`;
    div.onclick = () => selectAccount(a, regs);
    al.appendChild(div);
  });

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
  const isCur = a.uid === (all.find(x => x.lastLogin)?.uid || shortUid(a.uid));
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
function renderQuota(body) {
  const q = parseQuota(body);
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
  const soon = gift.filter(p => { const dl = daysLeft(p.deduction_end); return dl !== null && dl <= 30; }).sort((x, y) => daysLeft(x.deduction_end) - daysLeft(y.deduction_end));
  $('expire-note').innerHTML = soon.length ? '⏰ 30 天内到期：' + soon.map(p => `${p.name}（${daysLeft(p.deduction_end)}天，${fmt(p.deduction_end)}）`).join('；') : '近期无套餐到期。';
  window.__quota = q;
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
  const today = new Date().toISOString().slice(0, 10);
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
async function snapshotCurrent(uid) {
  try {
    const r = await invoke('snapshot_current');
    toast('已快照当前账号: ' + r.uid + '（' + (r.local_files ?? 0) + ' 文件' + (r.auth_included ? ' + 登录态' : '') + '）');
    refreshAccounts();
  } catch (e) { toast('快照失败: ' + e); }
}
async function switchTo(uid) {
  if (!confirm('确认切换到 ' + shortUid(uid) + '？\n切换会先退出 WorkBuddy，完成后需重启生效（参考 CC Switch）。')) return;
  try {
    const r = await invoke('switch_account', { uid });
    if (r && r.restart_required) {
      $('banner-msg').textContent = r.message;
      $('banner').classList.add('show');
      toast('已切换，请重启 WorkBuddy');
      refreshAccounts();
    }
  } catch (e) { toast('切换失败: ' + e); }
}
if ($('btn-restart')) $('btn-restart').addEventListener('click', async () => {
  try { await invoke('restart_workbuddy'); $('banner').classList.remove('show'); toast('已发送启动 WorkBuddy'); }
  catch (e) { toast('启动失败: ' + e); }
});

async function refreshAccounts() {
  let data;
  try { data = await invoke('list_accounts'); } catch { return; }
  // 把账号快照状态合并进侧边栏（list_accounts 不渲染 UI，这里只记录 has_snapshot）
  accountsCache.forEach(a => {
    const hit = (data.accounts || []).find(x => x.uid === a.uid);
    if (hit) a.has_snapshot = hit.has_snapshot;
  });
}

// ===== 数据加载 =====
async function loadAll() {
  $('raw-out').textContent = '请求中…';
  try {
    const j = await invoke('get_all');
    $('raw-out').textContent = JSON.stringify(j, null, 2);
    window.__login = j.login || {};
    window.__checkin = j.checkin || {};
    renderSidebar(j);
    renderAccount(j.login);
    renderJwt(j.jwt);
    renderEnv(j.env);
    if (j.quota && j.quota.status === 200) renderQuota(typeof j.quota.body === 'string' ? JSON.parse(j.quota.body) : j.quota.body);
    if (j.checkin && j.checkin.status === 200) renderCheckin(typeof j.checkin.body === 'string' ? JSON.parse(j.checkin.body) : j.checkin.body);
    if (j.memory && j.memory.status === 200) renderMemory(typeof j.memory.body === 'string' ? JSON.parse(j.memory.body) : j.memory.body);
    $('updated').textContent = ' · 更新 ' + new Date().toLocaleTimeString();
    refreshAccounts();
    loadModels();
  } catch (e) { $('raw-out').textContent = '错误：' + e; }
}
async function loadQuota() {
  $('raw-out').textContent = '请求中…';
  try {
    const j = await invoke('get_quota');
    $('raw-out').textContent = JSON.stringify(j, null, 2);
    if (j.status === 200) renderQuota(typeof j.body === 'string' ? JSON.parse(j.body) : j.body);
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
      const fullKey = m.apiKeyFull || m.apiKey || '****';
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

// 暴露给 inline onclick
window.addModel = addModel;
window.editModel = editModel;
window.delModel = delModel;
window.testModel = testModel;
window.testCurrentForm = testCurrentForm;
window.saveModel = saveModel;
window.closeModelModal = closeModelModal;
window.restartWB = restartWB;

// 暴露给 inline onclick
window.loadAll = loadAll;
window.loadQuota = loadQuota;
window.doCheckin = doCheckin;
window.openReport = openReport;
window.closeReport = closeReport;
window.copyReport = copyReport;
window.snapshotCurrent = snapshotCurrent;
window.switchTo = switchTo;

loadAll();
