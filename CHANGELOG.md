# 更新日志（Changelog）

所有版本均按语义化演进记录；日期为对应 tag 的提交日期。
格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，倒序排列。

> 💡 每次发布由 GitHub Actions 自动构建双平台产物（macOS `.dmg`/`.app.tar.gz`，Windows `.exe`(NSIS) + 便携版 `.zip`），见 [Releases](https://github.com/xmgzxmgz/workbuddy-account-hub/releases)。

---

## v0.6.31（2026-11-11）— 📥 tab 懒加载：额度/健康/记忆不再空白

- **用户反馈**：切到「额度 / 健康 / 记忆」三个 tab 都是空的，以为功能坏了。
- **真因**：这不是接口问题（实测额度/记忆/签到/宠物接口全部 200，Hub 侧 8 个账号 AT 均有效），
  而是这三个区块的数据**原本只在点按钮时才拉**（「预检全部」「查询全部账号记忆」「查询全部额度」），
  切 tab 不会触发任何加载，用户看到的就是空表。
- 修法：
  1. **tab 懒加载**：首次进入某 tab 自动拉对应分组数据（额度→全部账号额度、健康→预检、
     记忆→全部账号记忆、宠物→宠物状态、工具→模型列表），每个 tab 只自动拉一次。
  2. **额度加载失败不再静默**：原来 4 次重试全失败后什么都不显示（一片空白），
     现在把失败原因写进套餐表与到期提示，并指引「刷新全部 / 去健康 tab 预检」。
- bump 0.6.30 → 0.6.31

---

## v0.6.30（2026-11-09）— 🐾 宠物面板 403 修复 + 窗口放宽

- **宠物面板一直显示「无权限访问该接口（HTTP 403）」**：`call_api_as` 在 403 时统一抛
  「无权限访问该接口」，把成长系统「仅个人版可用」的原文吞掉了；而单账号宠物面板
  （buddy_status / buddy_config / buddy_depart / buddy_claim）用的是**当前登录态**，
  企业版身份必然 403。
- 修法：
  1. **四个单账号宠物命令全部接入 `growth_login_for()`**：当前登录态是企业形态时
     自动降级用同账号的个人版快照（与批量宠物功能一致）。
  2. `call_api_as` 的 403 分支**附上服务端原文**并识别
     `only available for personal users` → 直接给「宠物/成长系统仅个人版可用
     （该账号当前是企业版身份）」，不再有天书。
  3. 宠物面板标题标注数据来源：企业形态时显示
     `/activity/growth/buddy/travel · 企业形态，显示个人版宠物`，避免误以为企业版也有宠物。
- **窗口默认尺寸 1080×720 → 1360×900**，新增最小尺寸 980×640（此前过窄，内容挤成两行）。
- bump 0.6.29 → 0.6.30

---

## v0.6.29（2026-11-09）— 🚑 修复启动回归 + 落地 5 个 Roadmap 功能

**🔴 紧急回归修复：v0.6.28 装了但打不开**
- 现象：安装后进程立刻退出，用户看到"没装成功"。
- 根因：v0.6.27 把单实例锁改成 `create_new` 原子抢锁后，判活只靠 `pid + kill -0`。
  旧实例退出后 **PID 被系统复用**给别的进程 → `kill -0` 成功 → 新实例误判"已有实例在跑"，静默退出。
- 修法：锁文件加**心跳**（运行期每 10s 重写刷新 mtime）+ 新鲜度阈值 30s。
  只有「进程活着 **且** 心跳新鲜」才算真在运行；陈旧锁自动接管。崩溃/强杀也不会再卡住下次启动。

**新功能（全部落在「🧰 工具」tab + 「🐾 宠物」tab）**
1. **通知 Webhook**：Bark（iOS 推送）/ 企业微信群机器人。配置存本地，支持「发送测试」；
   战报与续期结果会自动推送。纯 HTTP 直连，失败不影响主流程。
2. **自动续期守护**（默认关闭）：每 30 分钟扫描，剩余 <7 天且**非当前登录**的账号自动续期，
   账号间 400ms 限速；跳过当前登录态（避免与客户端抢 refreshToken 轮换）。
3. **会话复制**（自 workbuddy-switch 启发）：把源账号的本地会话**复制**一份给目标账号，
   新会话带「（副本）」标记，**源账号不受影响**（与切换时的「搬移」语义相反）。
   复制前自动备份 workbuddy.db；同 uid 的两形态会拒绝（本就共用会话）。
4. **OpenAI 兼容网关**（自 workbuddy2api-hub 思路）：`POST /v1/chat/completions` 转发到官方
   `/v2/chat/completions`，支持 SSE 流式透传；`GET /v1/models` 列出账号。
   **仅监听 127.0.0.1:8787**，账号用请求头 `X-WB-Account` 选择（账号键或 uid 前缀），缺省用当前登录态。
   不做多账号轮询压榨（那才是风控高发区）。
5. **成长任务进度 + 一键领取**（自 wokbuddy-demon 启发，但**不做行为模拟**）：
   实测 `GET /activity/growth/tasks` 200、`POST /activity/growth/tasks/{id}/claim` 路由存在。
   任务完成由官方按真实使用行为判定（对话/装技能/链微信），Hub 只展示进度并领取**已完成**任务的奖励。
   —— 这是"一键完成 17/18 任务"在安全边界内的可行形态；真正代做任务需要模拟用户行为，不做。

**未实现（如实说明）**：Qoder CN 签到。本机虽装有 `Qoder CN.app`，但其 macOS 凭据是
Keychain + AES-GCM 加密（88lin 脚本里的 DPAPI 是 Windows 专属），在 Hub 内复刻需申请钥匙串权限
且随 Qoder 版本易碎；建议直接用 88lin/workbuddy-auto-signin 的 `qoder_signin.py`。

- bump 0.6.28 → 0.6.29

---

## v0.6.28（2026-11-09）— 🔌 全接口实测修复 + 顶栏功能 tab + 调研引入新功能

**1）全接口实测（14 个只读接口真实探针，5.9.3 客户端）**
- 结论：**接口一个都没被换掉**。此前"功能不可用"的观感来自两个真实原因：
  a) **当前登录态是企业版身份，成长系统（宠物）仅个人版开放 → 全部 403**；
  b) v0.6.24–0.6.26 的账号键漏改 bug（v0.6.27 已修）。
- **宠物接口修复**：批量宠物功能（签到外的成长类）对企业形态账号**自动降级用同账号的个人版快照 token**；
  无个人版快照则明确跳过。单账号 *_for 命令走 `growth_login_for()` 同样降级。
- **403 翻译**：`growth_post()` 把服务端 "growth system is only available for personal users"
  翻译成「宠物/成长系统仅个人版可用（该账号当前是企业版身份）」，带 `personal_only` 标记，
  不再显示"能量接口 HTTP 403"这类天书。
- 探针留档：`tests/probe_all_apis.rs`（ignore）覆盖全部接口 + 个人版身份对照。

**2）顶栏功能 tab（替代上下滚动找功能）**
- 顶栏下新增 7 个 tab：👤 账户 / ✅ 签到 / 📊 额度 / 🩺 健康 / 🐾 宠物 / 🧠 记忆 / 🧰 工具。
- 点击只显示对应分组区块，尊重折叠状态；顶部概览卡归「账户」tab。
- 经 headless DOM 断言与截图验证：切「宠物」tab 时仅宠物区块可见、tab 高亮正确。

**3）调研 7 个同类项目后引入的功能**（88lin/workbuddy-auto-signin、Javis603/token-monitor、
   changexbc/workbuddy-switch、BegoniaHe/wokbuddy-demon、ardeyouxipianyi/workbuddy2api-hub、
   jlcodes99/cockpit-tools、Xxx91n/jiahao）
- **邀请码与邀请人数**（GET my-code / invite-records，实测 200）：账户档案新增「邀请码」行，一键复制。
- **企业信用用量**（POST get-enterprise-user-usage，实测 200）：企业版形态在账户档案显示
  「企业信用：剩余 X · 已用 Y」。
- **今日战报**（启发自 wokbuddy-demon）：顶栏「📊 战报」按钮一键汇总账号数/登录态/最早到期积分/
  临期告警，推送通知中心并展示。
- **临期通知**：额度查询后 3 天内到期的积分包自动写入通知中心（此前只有页内切换推荐条）。
- 排查确认 Hub 已有同类功能不再重复开发：积分排行榜（rank 模式）、积分池智能调度（workbuddy-switch 同款）、多账号并行管理。
- 评估后**不引入**：成长任务自动完成（wokbuddy-demon 是重度模拟用户行为，风控高危）、
  网关转 OpenAI 协议（workbuddy2api-hub 明示上游收紧、账号风险自负）、应用多开（与切换器定位冲突）。
- Roadmap（后续版本）：会话复制（workbuddy-switch 王牌功能）、通知渠道 webhook（bark/企微）、
  Qoder CN 签到、自动续期守护。

- bump 0.6.27 → 0.6.28

---

## v0.6.27（2026-11-09）— 🛡 全面体检修复：正确性 × 安全 × 风控 × 体验（审计 Top 10 落地）

6 维度审计（安全/逻辑/交互/视觉/性能/代码质量）挖出的 10 项问题全部修复：

**正确性（🔴）**
- **main.rs 账号键漏改**：v0.6.24 改造只改了 account_ops 与 preflight，main.rs 的批量函数
  全部漏改（签到/额度/记忆/宠物/buddy 共 7 处 `a.uid`）——企业版账号的批量操作一直在拿
  个人版快照的 token。全部改为 `a.key`；签到去重键也从 uid 改为账号键（两形态各签一次）。
- **escapeHtml 补引号**：原来只转义 `&<>`，内联 onclick/title 属性存在注入面；补 `"` 与 `'`。

**风控（🔴）**
- **quota_all / buddy_all / pet_energy_all 限并发**：原实现 N 账号 = N 并发同时打网关
  （14 账号瞬发 14 个请求），与签到侧 250ms 限速策略自相矛盾；改为每批 3 并发。

**竞态与健壮性（🟠）**
- **快照轮询切换互斥**：切换进行中（WB 已杀、auth 半写）30s 轮询暂停，不再可能拍到中间态。
- **单实例锁原子化**：读-判-写三步改 `create_new`（O_EXCL）一步抢锁，消除 TOCTOU 双开窗口。
- **vault 保洁**：每个账号键 history 只保留最近 20 份；回收站超 30 天自动清理
  （此前无任何上限，磁盘无限膨胀、备份列表越来越慢）。
- **忽略表语义修正**：删除企业版不再连带隐藏同账号的个人版（只按账号键匹配）。
- delete_account 当前账号保护改为「当前登录形态」：企业版会话下删除个人版形态属合法操作。

**体验（🟠/🟡）**
- **B 方案落地**：全部账号×3、宠物能量、AI 记忆、API 管理、用量历史 7 个低频区块默认折叠
  （标题带 ▸/▾，点击展开，渲染逻辑零改动），首屏只剩核心四区。
- **5 处原生 confirm() 全部替换**为站内 confirmModal（深色卡片风格、多行后果说明、防重入）。
- 预检「国际版」徽标统一复用 `.badge.ent` 组件（消除两套蓝色/圆角各表其政的问题）。

**代码质量（🟡）**
- wb_api 6 个 warning 清零（last_err 死变量、多余 mut）；deprecated API_BASE 与
  `_api_base_default` 死代码删除。
- 测试更新：删除测试改用 current_account_key 断言保护语义。
- bump 0.6.26 → 0.6.27

---

## v0.6.26（2026-11-05）— 🐞 修复：账号键混入会话库，导致切到企业版后对话全空

- **现象**：切到企业版账号后，客户端一条对话都不显示。
- **两个原因叠加**：
  1. **v0.6.24 埋的隐患（本次修复）**：`switch_account` 的参数变成账号键（企业版形如
     `<uid>@<eid>`），若直接用它改写 `sessions.user_id`，客户端按纯 uid 查询会一条都找不到。
  2. 该账号本身本地会话为 0（实测 A 账号名下 0 条，B 账号名下 336 条）。
- **修法（key 与 uid 严格分离）**：
  - vault 路径、`switch_auth_to` 用 **key**
  - `workbuddy.db` 的 `sessions.user_id`、`local_storage` 合并、置顶状态迁移用**纯 uid**
  - 搬移判断改用纯 uid 比较：**跨账号切换照样搬**（B 账号 → A 账号，会话跟着过去），
    而**同一账号的个人版 ⇄ 企业版互相切换不搬**（同 uid，本来就共用同一批对话）
- **语义确认**：切换仍是「搬」不是「复制」——搬走后源账号看不到这些对话，回滚用
  `vault/<cid>/history/<ts>/workbuddy.db.bak`。
- bump 0.6.25 → 0.6.26

---

## v0.6.25（2026-11-04）— ⏱ 登录态形态定时兜底（避免「登录了企业版但没被保存」）

- **实际踩到的问题**：用户在客户端登录企业版 → Hub 没重启 → `ensure_snapshot` 不会跑 →
  这份企业版登录态从未落盘 → 用户切回个人版后，企业版 token 直接丢失，等于白登录一场。
- **修法**：`ensure_snapshot` 改为每 **30 秒**轻量检查一次当前账号键。
  它内部只在「该键还没有快照」时才真正写盘，所以是幂等的，开销只有一次 IPC + 一个文件判断。
- 新建快照时会 `bootLog` 提示并自动刷新侧栏，新形态（如企业版）立刻出现，不需要重启 Hub。
- ⚠️ 局限：Hub 必须处于运行状态才能兜住。若 Hub 未开时登录企业版又切走，仍会丢失——
  这种情况只能重新登录一次（客户端只有一个登录态槽位，切走即覆盖）。
- bump 0.6.24 → 0.6.25

---

## v0.6.24（2026-11-04）— 🏢 支持同一账号的个人版 / 企业版并存（账号键改造）

用户场景：同一个 OneID 账号（13800000000 / `a1b2c3d4`）在网页登录时可选择个人版或企业版，
两者**uid 完全相同**，此前 vault 以 uid 作目录名，两种形态共用一个槽位，切一次覆盖一份。

- **实测数据**（企业版三要素）：`account.type` = `personal` → **`ultimate`**；
  `enterpriseId` 从无到 **`example-enterprise`**；`idp` = `phone`。uid 不变。
- **vault 目录改为账号键（accountKey）**：
  - 个人版 `vault/<uid>`
  - 企业版 `vault/<uid>@<enterpriseId>`
  - 旧目录（纯 uid）天然继续表示个人版，向后兼容
- **新增 `account_key` / `parse_key` / `key_of_account` / `current_account_key`**，
  `list_accounts` / `switch_account` / `snapshot_current` / `ensure_snapshot` / `backup_all` /
  `refresh_account` / `delete_account` / `preflight_accounts` 全部从 uid 改为按 key 运作。
- **关键修复**：`snapshot_current` 保存当前登录态时用 `current_account_key()` 决定落盘目录 ——
  这是「切一次覆盖一份」的根因所在。
- **自动数据迁移** `migrate_legacy_snapshots`：扫描历史遗留的「裸 uid 目录但快照内容是 ultimate」，
  改名到 `<uid>@<eid>`；目标已存在时保守跳过，永不覆盖。
- **个人版槽位自动找回**：迁移后若 `<uid>` 目录缺失，从 `<uid>@<eid>/history/` 里挑最近一份
  personal 备份重建个人版槽位 —— 用户不必为了切回个人版再去客户端重新登录。
  （本地实测：个人版 AT 29.8 天、企业版 AT 30.0 天，两条各自独立有效。）
- **UI**：企业版账号带蓝色「企业」徽标，副标题显示 `uid · enterpriseId`；
  个人/企业两行并存；顶栏账号类型显示「企业版」；星标与标签挂在 uid 上（两形态共享），
  删除则针对具体形态。
- **测试** `tests/account_key.rs`（5 个，CI 执行）：键拼装/解析、空串与空白 enterpriseId 归一化为个人版、
  account JSON 取值、遗留目录迁移、迁移幂等且不覆盖既有数据。
- bump 0.6.23 → 0.6.24

---

## v0.6.23（2026-11-04）— 📐 修复侧栏账号行被操作按钮挤爆

- **现象**：账号行里昵称被压成两行、UID 只剩「xxxx...」、手机号被截断显示。
- **根因**：行内并排了 4 个 `flex-shrink:0` 元素（dot 27 + 切换按钮 48 + ★/🏷/🗑 共 74 + gap），
  把 `.info` 压到只剩 **66px**；而手机号掩码本身就要 ~70px，必然溢出成省略号。
- **修法**：把 ★ / 🏷 / 🗑 收进一个「⋯」浮动菜单，行内只留「切换 + ⋯」，
  `.info` 恢复到 ~118px；星标状态改用名字前的 ★ 前缀表达，标签改用 `title` 悬停提示，信息不丢。
- 菜单为 `position:fixed` 浮层，贴按钮右侧展开并做窗口边界收敛，点空白处关闭；
  当前登录账号的菜单里不出现「删除」项。
- bump 0.6.22 → 0.6.23

---

## v0.6.22（2026-11-04）— ✅ 修复 CI：删除测试里有一个依赖本机数据

- v0.6.21 的 `real_vault_still_lists_accounts` 断言「真实 vault 至少有一个账号」，
  但 CI runner 上没有真实账号数据，导致 `cargo test` 在 CI 里必红（本地却是绿的）。
  已标 `#[ignore]`，并写清原因，本地可手动 `-- --ignored` 跑。
- **v0.6.21 的安装包本身没有问题**（只是 CI 里一个测试的断言不适用于空环境），
  但如果要追求「代码与 CI 全绿」，用本版即可。
- bump 0.6.21 → 0.6.22

---

## v0.6.21（2026-11-03）— 🗑 删除账号（失效账号清理，可还原）

需求：「有的账号失效了得删掉」。此前只能看着它一直躺在列表里。

- **侧栏账号行新增 🗑**（仅非当前账号显示）。删除后立即从列表消失。
- **删除是「可后悔」的，不是 `rm -rf`**：
  1. 当前登录账号**直接拒绝**（删了只会让列表和真实登录状态脱节，提示先切换）
  2. 快照 rename 到 `vault/.trash/<uid>.<ts>/`（同盘瞬时、不占额外空间，历史备份一起搬走）；
     跨设备时回退到复制后删除
  3. uid 记入 `~/.workbuddy-account-hub/ignored_accounts.json`
- **为什么必须记忽略表**：账号列表是三处合并（`auth.allAccounts`、`local_storage` 登记表、vault 快照），
  只删 vault 目录的话另外两处仍会把它吐出来，表现为「删了还在，只是变成未登录」。
- **还原入口**：⚙ 设置 →「已删除账号」→ 还原（解除隐藏 + 把回收站里最近一份快照移回原位）。
- **`vault_account_uids` 排除 `.` 开头目录**，否则 `.trash` 里的残留会被当成账号扫回来。
- **测试** `tests/delete_account.rs`（4 个，CI 执行）：删除→进回收站→列表过滤→还原全链路、
  当前账号被拒、无快照账号只隐藏、真实 vault 不被误伤。忽略表是全局文件，
  测试用全局 Mutex 串行化避免并行 flaky，并在 Drop 里备份/还原本机配置。
- bump 0.6.20 → 0.6.21

---

## v0.6.20（2026-11-03）— 🌏 支持海外版（WorkBuddy AI）：按账号域名选网关

回应 issue #4「可以支持下海外版吗」。

- **根因**：`API_BASE` 是写死常量 `https://copilot.tencent.com`，全部 17 处请求都往国内网关打。海外版账号的凭据属于国际区，打国内网关必然 401/403 —— 所以此前不是"没适配"，而是"用错了网关"。
- **改为按账号自己的 `auth.domain` 推导网关**：
  - `www.workbuddy.cn` / `copilot.tencent.com` / 空 → `https://copilot.tencent.com`（国内）
  - `workbuddy.ai` / `codebuddy.ai`（含大小写、空白）→ `https://www.workbuddy.ai`（国际）
  - 国际版两个品牌域（桌面端 workbuddy.ai、CodeBuddy CLI codebuddy.ai）共用同一国际网关，凭据互不通用
- **`LoginInfo` 新增 `domain` 字段**，`load_login` / `login_from_file` 都从 `auth.domain` 读取；`api_base(login)` / `api_base_for_domain()` / `region_of_domain()` 统一收口，17 处调用点全部替换。
- **`get_memory()` / `get_checkin()` 改为委托 `*_as` 版本**，避免无 login 上下文时退回国内网关。
- **补海外版的数据目录与可执行文件候选**（其目录名、进程名均不同）：
  - 登录态：`WorkBuddyAI` / `WorkBuddy AI` 数据目录、`~/.workbuddy-ai`、`%LOCALAPPDATA%\WorkBuddyAI`
  - exe：`WorkBuddyAI.exe`（进程检测、注册表卸载项、`%LOCALAPPDATA%\Programs\WorkBuddyAI`），手选校验同步放宽
- **UI**：🩺 登录态健康面板给海外账号打「国际版」标签，预检结果附带 `region` / `domain`。
- **测试**：
  - 新增 `tests/region_mapping.rs`（4 个纯逻辑用例，CI 执行）：国内/国际域名映射、大小写与空白容错、未知域名回退国内（保证存量用户不被误切）。
  - 新增 `tests/region_real.rs`（ignore，需真实登录态）：读本机 `auth.domain` → 推导网关，并断言国内账号**没有**被误判成国际版。
  - 回归：国内 5 个账号预检仍全部正常（`ok:4 / revoked:1`，其中 revoked 是已知废号），无行为变化。
- **未验证项（需海外账号配合）**：国际网关下 billing / 签到 / 宠物各接口的路径与返回结构是否与国内一致。若海外账号某接口报 404/字段缺失，属路径差异，请把返回体贴回来即可按域调整。
- bump 0.6.19 → 0.6.20

---

## v0.6.19（2026-10-30）— 🧹 清理历史遗留脚本与死代码（仓库瘦身）

纯清理，无功能变更，无行为风险。

- **删除根目录 5 个 `ci_wait*.py`**（729 行里的主体）：本质是同一个「等 CI 完成」轮询脚本复制 5 份改版本号，CI / 构建脚本 / README 全都不引用。
- **删除 `detect.py` / `switch.py` / `vault.py`**：早期 Python 版运维脚本，功能已全部由 Rust `account_ops` 重写接管；README 的目录说明同步更正。
- **删除 `sync/`（28K）与 `credits/`（88K）**：跨机同步与计费接口的早期 JS 原型，主程序零引用；正式同步已是 `account_ops/src/sync.rs`。
- **删除 4 个从未被调用的 Tauri 命令**：`quota_for`、`memory_for`、`models_path`、`restart_self`。前端 `main.js` + `index.html` 对它们 0 引用，且无动态拼接命令名的调用路径（批量版 `quota_all` / `memory_all` 已覆盖需求）。
- **删除死函数 `buddyLogInit()`**：从无调用点，且 `buddyLogAdd` 直接把新条目 prepend 进容器、历史本就保留在 DOM 里，重建无意义。
  - （附带修正一处早先误判：`id="buddy-log"` 容器**是存在的**，`buddyLogAdd` 有 33 处调用，宠物操作日志是有效功能，只删掉多余的重建函数。）
- 净效果：**删除 729 行 Python + 116K 原型 JS + 41 行 Rust + 13 行 JS**，仓库文件数 103 → 88，README 与实现对齐。
- **移除「跨机同步」功能**（按需求下线）：删除 `account_ops/src/sync.rs`（677 行）、`sync_roundtrip` 测试（128 行）、4 个 Tauri 命令（`sync_status`/`sync_push`/`sync_pull`/`sync_set_dir`）、前端同步面板与 4 个函数（58 行）、`zip` 依赖。界面少一个 section。
- **顶栏瘦身**：从 8 个同权重按钮降到 4 个主操作（刷新全部 / 一键签到 / 👁 隐私 / ⚙ 设置），低频操作（仅查积分、宠物面板、导出报告、历史备份）收进新增的「⋯」下拉。
- **调试面板默认隐藏**：「原始返回（调试）」改为 `display:none`，需要排查时从「⋯」展开并平滑滚动定位，不再占据页面底部。
- bump 0.6.18 → 0.6.19

---

## v0.6.18（2026-10-30）— 🧩 修复侧栏「当前登录」账号名被头像挤压/裁切

回应 issue #3 的 Windows 反馈第 2 点。

- **两个叠加的根因**：
  1. 头像 `.av` 用的是 `float:left`，而 `.nm` / `.id` 仍是普通块级元素——float 只让**文字**环绕，块级元素自身宽度还是整行宽，于是名称从容器左边缘起排、被头像压住，右侧越界被裁；
  2. 更隐蔽的一层：`.sidebar` 是 `display:flex; flex-direction:column`，子项默认 `flex-shrink:1`。当侧栏内容总高超出视口（窗口变矮 / 账号变多）时，**`.me` 卡片自身被压缩**，叠加 `overflow:hidden`，把名称第二行、UID、类型 badge 直接裁掉——这才是「窄窗口下更明显」的真正原因，不只是横向挤压。
- **修法**：① 头像 `flex:0 0 36px` 固定不压缩，文字区改为 `.me-info` 容器 + `flex:1 1 0; min-width:0`（只有这样才会真正让位并按需换行），名称 `overflow-wrap:anywhere`、UID `word-break:break-all`；② 给 `.sidebar > *` 加 `flex-shrink:0`，让侧栏整体滚动而不是压缩子项；③ badge 移入文字列，`align-self:flex-start` 保持紧凑不拉伸。
- **验证**：用真实 `index.html` 去掉 Tauri 依赖后 headless 渲染两个极端场景（`Change979` / `Change979-超长账号名称测试` + 41 字符 UID），短名称与超长名称均完整显示、超长自动折行、badge 位置正常，无任何裁切。
- bump 0.6.17 → 0.6.18

---

## v0.6.17（2026-10-30）— 🩺 签到前三态预检 + ♻️ refreshToken 静默续期（RT 可无限保活）

回应 issue #3 的问题 B / C。两个功能都已用真实账号实测跑通，并纠正了一条关键协议细节。

- **问题 B：签到前预检 + 三态标注**
  - 新增 `wb_api::preflight_login`：本地按 JWT `exp` 判是否到期 + 用只读接口 `checkin-activity-status` 探一次服务端真实接受度 → `ok` / `expiring`（<7 天）/ `revoked`（JWT 未到期却 401）/ `expired` / `error`。
  - **优先级：已到期 > 吊销 > 将过期 > 正常**——被吊销的账号即使还剩 14 天也用不了，必须先于「将过期」显示。
  - `checkin_all` 改为**先预检再签到**：`revoked`/`expired` 直接跳过并标 `needs_relogin`，不再白发一次必失败的签到请求；顺带复用预检返回的 `today_checked_in` 省掉一次重复请求。
  - 签到结果表把这类账号单独渲染成「需重新登录」红标，与频控/网络错误彻底区分；完成提示追加「需重新登录 N」。
  - 新增 🩺「登录态健康」面板：预检全部 + 每账号状态/AT 剩余/说明/单账号续期按钮。
- **问题 C：refreshToken 静默续期**
  - **协议实测澄清（关键）**：端点不是 `POST /v2/auth/token/refresh`（404），而是
    **`POST https://{auth.domain}/v2/plugin/auth/token/refresh`** —— `prefixPath = /plugin` 不能省。
    headers：`X-Domain`、`X-Refresh-Token`、`X-Auth-Refresh-Source: plugin`。
  - **实测结论（最能决定方案价值的一条）**：续期成功后 **refreshExpiresIn 会被重置为满 60 天**（实测 RT 59.9 → 60.0 天）。
    也就是说 RT 寿命是**顺延**的，只要在 RT 到期前持续续期就能无限保活，不是「补满 AT 但不延长总寿命」。
  - `account_ops::refresh_account` 四道保险：① 续期前把快照 `auth.info` 备份到 `history/<ts>/`；② 目标是当前登录账号且客户端在跑时**直接拒绝**（避免和客户端抢同一个 RT 把客户端踢下线）；③ 只覆盖服务端确实返回的字段（响应未带 refreshToken 时保留旧值，绝不把 RT 清空）；④ 重新封信封 + 原子写，写失败明确告知「需重新登录」。
  - 新增 ♻️「批量续期」：只处理 `expiring`/`revoked` 且非当前登录的账号，串行 + 800ms 间隔防风控。
- **验证（真实账号，macOS 5.8.3）**
  - 三态预检：5 个账号 → `{ok:4, revoked:1}`，其中之一确认为「JWT 被吊销」型 401。
  - 续期：另一账号实测成功，AT 29.9 → 30.0 天，RT 60.0 天，`rotated=true`，快照回写后 token 仍是 `$wbEncrypted` 信封、无明文残留。
  - 反例：该账号续期失败 `invalid_grant: Session doesn't have required client` —— 该账号的 session 已被服务端连 RT 一起清理，**只能重新登录**，这印证了「401 账号未必能靠续期自救」。
- **新增测试**：`account_ops/tests/preflight_real.rs`（三态分布，纯只读）、`account_ops/tests/refresh_real.rs`（`REFRESH_UID` 指定目标，未设置则跳过）。
- bump 0.6.16 → 0.6.17

---

## v0.6.16（2026-10-30）— 🔑 5.8.3 切换失效根治：加密回写 + 活跃登录态择优

回应 issue #3（客户端 5.8.3 后切换账号失效）。实测确认 issue 给的两个归因都不是真因，真正的两个根因是：

- **根因一（新发现，最致命）：写回明文 token 到加密登录态文件。** 客户端 5.8.3 把 `accessToken`/`refreshToken`/`nickname`/`phoneNumber` 以 `{"$wbEncrypted":1,"envelope":".."}`（AES-256-GCM 字段信封）加密落盘，加密由主进程 credential-protection 存储层在原生层统一执行（app.asar 的 JS 层 grep 不到 `$wbEncrypted`），即**客户端按信封格式读取**。而 Hub 只有解密没有加密，`switch_auth_to` 解密后就地序列化写回 → 文件里躺裸明文 → 客户端判字段损坏 → 读不出登录态 → 表现为「切换报成功，账号毫无变化」。
  - 新增 `wb_enc::encrypt_field` / `selftest_roundtrip`：AES-256-GCM + 官方 AAD 帧构造，keyId = `sha256(key).hex[..16]`（= keyblob `protectorKeyId`），nonce 12B 密码学随机（`getrandom`），密文不带 WBE 帧前缀、tag 单独落盘 —— 逐字节对齐客户端真实文件格式。
  - 新增 `wb_api::seal_sensitive_fields`：写回前把敏感明文字段封回信封（已是信封的跳过，不二次加密），覆盖 `auth.*`、`account.*`、`accounts[]`、`allAccounts[]`。
  - `switch_auth_to` 改为：**以登录态文件自身既有形态为准**决定是否封装；`at_rest_seal_ready()` 不通过或封装失败时**中止切换、绝不写明文**；落盘改原子写（临时文件 + rename），避免客户端读到半个 JSON。
- **根因二：写回目标不是客户端真正在读的文件。** 5.8.3 同机可能并存精确名 + 一批 `<name>.<ts>.<pid>.<uuid>.info` 轮转备份，落盘目录也随版本变动；旧逻辑「硬编码顺序取第一个存在的」会稳定选中已废弃的陈旧文件。
  - 新增 `wb_api::active_auth_file()`：候选集（硬编码 + 目录扫描兜底，去重）→ 只留有 `auth.accessToken` 的 → **精确名 `workbuddy-desktop.info` 优先 → mtime 最新**。
  - `account_ops::auth_file()` 与 `wb_api::load_login/load_auth_file` 统一走这套裁决，读和写不再各用一套逻辑。
  - 新增 ⚙ 设置面板「登录态路径诊断」：列出全部候选（精确名/含token/mtime/大小）、高亮命中的活跃文件、显示回写能力（可加密/无密钥），一键定位「切换无效」。
- **附带澄清 issue 的一个错误建议**：issue 提议 fallback 到 `~/.workbuddy/local_storage/wb_entry_*.info`。实测该文件是 `[{userId,data,ts}]` 配置快照（models/featureToggles 等，300KB+），**不含任何 token**，不是登录态；写进去只会破坏配置。勿采纳。
- **验证**：新增 `tests/atrest_seal_roundtrip.rs`（3 个真实数据测试）——① 纯算法往返 ② 真实 token 解密→重封→解回一致且 keyId == keyblob ③ 整文档 6 个敏感字段全部信封化、明文不残留、逐字段无损。macOS 5.8.3 实机全绿。
- bump 0.6.15 → 0.6.16

---

## v0.6.13（2026-10-08）— ⚙ 设置面板：WorkBuddy.exe 自动检测优先 + 手选兜底

回应 issue 反馈（用户不希望用环境变量指定自定义安装路径）：

- **自动检测（新增，优先）**：①正在运行的 WorkBuddy 客户端进程真实路径（客户端开着即可零配置抓到，任意安装盘）→ ②注册表 NSIS 卸载信息 InstallLocation（HKCU/HKLM/WOW6432Node）→ ③标准安装路径。昂贵检测带 60s TTL 缓存，GUI 子进程一律 `CREATE_NO_WINDOW` 防黑框
- **手选兜底（新增）**：顶栏 ⚙ 设置面板 —— 显示当前生效路径与检测状态；「浏览…」原生文件选择框（rfd）或手动粘贴；保存即校验文件存在并立即重载（`hub_settings.json` 与 hub exe 同目录，每次解密现读不缓存）；「恢复自动检测」一键清除手选
- **失败提示改版**：「自动解密失败」横幅直接给「打开 ⚙ 设置」链接，不再让用户敲 setx；`WORKBUDDY_EXE` 环境变量保留为最低优先级向后兼容
- bump 0.6.12 → 0.6.13

---

## v0.6.12（2026-10-08）— 真正根治「自动解密失败」：token 回填键名错位修复

**这是加密客户端下「自动解密失败」的真正根因**——v0.6.7 引入解密以来，token 回填从未真正生效过：

- **Bug**：`decrypt_envelope_fields` 采集 token 时用裸键名 `accessToken`/`refreshToken`，而回填循环按 `auth.`/`account.`/`allAccounts.` **前缀分发** → 裸名 token 永远匹配不上，**从未被回填**。nickname/phone 带前缀能回填 → 函数返回 true → `load_login` 从仍是密文信封的 accessToken 上取不出字符串 → 返回 None → 整个 UI 显示「⚠ 登录态已加密且自动解密失败」。
- **为何一直没发现**：解密 worker 本身完全正常（v0.6.11 黑匣子 194 次调用全 OK、python 复现测试全过）——所有测试都验证了 worker 层，唯独没验证 Rust 侧「解密结果 → 文档回填」这一步。
- **修复**：采集键改为 `auth.accessToken`/`auth.refreshToken`，与回填前缀规则一致。该修复同时打通 `switch_auth_to` 的加密快照切换（同样依赖此回填）。
- **加固**：`load_login` 每个失败分支（解密失败 / token 空 / uid 空）与成功路径（uid、token 长度）写入黑匣子日志 `decrypt_debug.log`，今后「未登录」原因一眼可见。
- bump 0.6.11 → 0.6.12

---

## v0.6.11（2026-10-08）— 冷启动解密失败修复 + 解密黑匣子日志

> 现象：hub 冷启动首次加载偶发「登录态已加密且自动解密失败」（10:02、11:08 两次复现），稍后手动刷新又正常。外部复现全部失败：python 同命令/同环境/并发 4 路/计划任务上下文均 100% 成功。

- **根因**：经计划任务等上下文启动时 hub 的 cwd 是 `C:\Windows\system32`，spawn 官方 WorkBuddy.exe 未显式设 cwd → 子进程继承 System32 → 冷启动场景下解密 worker 必失败；v0.6.9/10 的重试同样继承坏 cwd，故重试无效
- **修复**：worker spawn 显式 `current_dir` = 官方 exe 所在目录；重试升级为 3 次（400ms/900ms 退避）；`LOCALAPPDATA` 缺失时从 `USERPROFILE` 推导兜底
- **解密黑匣子**：每次 worker 调用把候选列表/cwd/spawn 错误/退出码/stdout·stderr 尾部/耗时写入 `<hub 目录>/decrypt_debug.log`，装机实测冷启动 8 次调用全部 attempt 0 通过（0.1–0.4s）
- bump 0.6.10 → 0.6.11

---

## v0.6.10（2026-10-08）— 「30 天内到期」展示重构：摘要 + 芯片 + 折叠

> 原实现把几十个待用包用「；」串成一行文字墙，几十行铺满屏幕，可读性差。

- **摘要一行**：`⏰ 30 天内到期 N 个包 · 共余 X · 最近 MM-DD HH:mm（N 天后）`，一眼看清全局
- **紧凑日期芯片**：每个包一枚芯片（`MM-DD 余X`），≤3 天到期的标红；默认只展示最近 **6** 个，其余收进「▸ 展开全部 N 个」，点击切换显示
- 悬停芯片显示完整信息（套餐名 / 精确到期时间 / 天数）；过滤与 resource_id 去重逻辑不变
- bump 0.6.9 → 0.6.10

---

## v0.6.9（2026-10-08）— 解密健壮性：重试 + 超时（消除升级/重启瞬间的「自动解密失败」）

> 现象：客户端 5.7.6 升级完成后首次打开 Hub，偶发「登录态已加密且自动解密失败」（10:02:58 一次实测复现），手动重试即恢复——系客户端重写登录态落盘瞬间的一次性 spawn/读取竞争。

- **解密 worker 重试**：`run_wb_decrypt_worker` 对每个候选 exe 最多尝试 2 次（spawn 失败 / 非零退出 / 超时均触发重试），瞬时失败不再直接把「自动解密失败」抛给用户
- **20s 超时兜底**：worker 偶发卡住时 kill 并进入下一候选/重试，不再无限等待拖死后端加载
- bump 0.6.8 → 0.6.9

---

## v0.6.8（2026-10-08）— 加密快照切换打通 + 自定义安装路径支持

> 反馈来源：GitHub issue #1（@change979666）——v0.6.7 下加密快照被 switch_auth_to 直接拒绝，未接入已有解密能力。

- **切换账号支持加密快照**：`switch_auth_to` 在校验前先调用 `wb_api::decrypt_envelope_fields` 对快照做**内存解密**（走官方 WorkBuddy.exe node 模式取钥，绝不回写快照文件），解出明文 token 后走原有校验/写回链路；解密失败（官方 exe 不可达等）才报错，且错误信息给出可操作建议
- **自定义安装路径支持**：`wb_api::wb_exe_candidates` 与 `account_ops::workbuddy_exe` 均优先读取 `WORKBUDDY_EXE` 环境变量（指向 WorkBuddy.exe），解决 D 盘安装 / junction 映射等场景下解密与重启客户端双双失败的问题；顺带补齐 `ProgramFiles(x86)` 候选
- **前端提示更新**：账户档案区加密检测文案更新为「解密失败 + WORKBUDDY_EXE 设置指引」，替代 v0.6.6 时「无法解密」的过时描述
- bump 0.6.7 → 0.6.8

---

## v0.6.7（2026-09-20）— $wbEncrypted 信封自动解密（加密登录态全功能恢复）

**背景**：v0.6.6 实现了加密登录态的检测与提示。本版进一步实现**自动解密**，加密环境用户的全部功能（额度/签到/记忆/切换）恢复可用。加密体系规则逐行取自官方 `app.asar` 的 `at-rest-crypto` chunk（勿凭记忆改动），并与 88lin/workbuddy-auto-signin#7、WorkDaddy#244 的同类实现相互印证：

- **解密实现**：`wb_api::decrypt_envelope_fields` —— 识别登录态（内存视图）中的 `$wbEncrypted` 字段信封（accessToken/refreshToken/昵称/手机号），整段解密脚本跑在官方 WorkBuddy.exe（`ELECTRON_RUN_AS_NODE=1` node 模式）内：native binding 取编译期 `atRestSecretKey` → `sha256` 推导 protectorKey 与 keyId → 按官方 AAD 规则（`WB-AAD` domain + WBEV1 + sym-v1 + field framing）AES-256-GCM 解密。密钥不落盘、不出进程；**绝不回写原登录态文件**
- **load_login 集成**：accessToken 为信封时自动解密，解出 token 后走原有全链路（额度/签到/记忆/仪表盘全部恢复）；解密失败（无官方 exe / 规则失效）回退 v0.6.6 的明确提示
- **get_all 集成**：账号列表昵称/手机号为信封时批量解密展示，不再出现乱码/空昵称
- **本地自测通过**：官方规则加密 → 解密往返一致（含中文昵称），keyId 校验 `9127dea1…`
- 已知未验证点：加密客户端环境下，Hub 切换账号写回的明文快照能否被新版客户端接受（本机无法构造该环境）；切换校验对加密 token 快照仍拒绝（v0.6.6 行为）
- bump 0.6.6 -> 0.6.7

---

## v0.6.6（2026-09-20）— 新版客户端加密登录态（$wbEncrypted）检测与兼容提示

**背景**：部分用户反馈 Hub 显示「未找到登录态 / 无本地数据目录」。排查确认：新版官方客户端把登录态文件的 `auth.accessToken` 改为加密存储——值从 `eyJ...` 明文字符串变为 `{"$wbEncrypted":1,"envelope":"..."}` dict。Hub 的 `accessToken.as_str()` 取不到值，被误判为「无 token / 未登录」。加密密钥在客户端内部，Hub 无法解密，本版做精准检测与明确提示（不再误导排查）：

- **get_all 新增 `auth_encrypted` 字段**：检测各候选登录态文件的 accessToken 是否为 `$wbEncrypted` dict，命中时返回文件路径
- **账户档案明确提示**：检测到加密登录态时显示「登录态已加密（$wbEncrypted，新版客户端行为）+ 处理建议」，替代误导性的「未找到登录态」；启动日志同步记录
- **切换校验加固**：切换目标快照若为加密 token 格式，明确报错说明原因（Hub 无法解密、写回后客户端也无法使用），不再笼统报「没有保存过登录态」
- bump 0.6.5 -> 0.6.6

---

## v0.6.5（2026-09-20）— 无积分包全站隐藏

- 「最早到期」只统计**还有剩余积分（remain>0）**的积分包；已用尽的包到期即失效，不再参与最早到期计算（单账号视图、全部额度表、导出 MD 一致）
- 已用尽的积分包在**所有位置隐藏**：单账号套餐明细表、到期时间轴、包计数（赠送包 X 包 / 体验版 X 个 / 套餐数）、导出 MD 套餐明细
- 积分池调度建议不受影响（acctEarliest 本就只统计 remain>0 的包）
- bump 0.6.4 -> 0.6.5

---

## v0.6.4（2026-09-20）— 积分池智能调度（提醒式）

- **feat：全部额度表新增「积分池调度建议」横幅。** 全池自动计算每个账号「remain>0 且未过期」的最早积分包（排除不限量/体验版），当推荐账号比当前登录账号**早到期 ≥24h**（或当前账号无紧迫到期积分）时，顶部提示 + ⚡一键切换（复用现有切换：自动备份当前账号 → 优雅退出 WorkBuddy → 换入目标登录态 → 自动重启）。
- **feat：额度表每行新增「切换」按钮**，当前登录账号显示「当前」徽标，任意账号一键对话切换。
- **feat：排名按钮升级三态**：关闭 → 按剩余额度 → **按最快过期**（积分池调度视角）。
- 保守策略参考 [changexbc/workbuddy-switch](https://github.com/changexbc/workbuddy-switch)：24h 阈值避免 prompt 缓存（按账号+会话维度）频繁失效造成隐性浪费；提醒式不打断正在进行的对话，由用户确认后切换。

## v0.6.3（2026-09-20）— main 分支收拢正式版

- **chore：`ui-redesign` 分支合入 `main` 后的首个正式构建**（fast-forward `44b4dbb` → `e85e974`，零冲突）。
- 代码与 v0.6.2 相同（Aurora Glass 界面 + 额度显示修复 + 性能优化），本次发版使 `main` 分支拥有独立于工作分支的正式 Release 产物；远端 `main` 同步推送（`68852a2` → `e85e974`）。

## v0.6.2（2026-09-09）— 性能优化：告别窗口冻结

- **perf：修复「很卡」的根因——全部网络命令从同步改为 `async + spawn_blocking`。**
  Tauri 2 同步命令在主线程执行，网络请求（超时上限 15s/请求）期间整个窗口冻结（点击/滚动全部无响应）。v0.5.35 起启动即拉真实额度数据，感知明显。本次 22 个命令全部后台线程化，UI 永不为网络等待而卡死。
- **perf：多账号批量命令并行化**（`quota_all` / `pet_energy_all` / `buddy_all_status`）：
  原串行逐账号请求（5 账号 × 每账号最多 15s），现 `std::thread::scope` 逐账号一线程并发，总耗时 = 最慢单账号（通常 1~3s）。
  `checkin_all` 保持串行 + 250ms 限速（防风控）。
- **perf(css)：移除大面积滚动区域的 `backdrop-filter`**（侧边栏 22px / 主面板 16px / 顶栏 18px / 吸顶表头 8px），改用高不透明度实色底（视觉几乎无差）；弹窗 / Toast / 小卡片保留玻璃感。滚动时不再每帧重算模糊。

## v0.6.1（2026-09-08）— 三个额度显示修复

- **fix(billing)：新计费三接口把 `DeductionEndTime` 从字符串日期改为毫秒时间戳数字**，旧解析只认字符串导致「最早到期」全显示「长期」+「近期无待用套餐到期」误报。新增 `first_time_val` 兼容两种类型（数字按本地时区格式化），实测 43 条套餐记录全部解析出真实日期。
- **fix(ui)：额度趋势图峰值/日期文字挪出被 `preserveAspectRatio=none` 拉伸变形的 SVG**，改为 HTML 信息行（峰值/当前/区间/记录天数），线条加 `vector-effect=non-scaling-stroke`。
- **fix(ui)：登录态「永远剩 60 天」说明**——实测 JWT `iat` 每日随客户端重新认证自动续签（Keycloak 机制），剩余天数重置为官方行为而非显示错误，界面增加如实说明。

## v0.6.0（2026-09-08）— Aurora Glass 界面重构

- **ui：全新视觉层重写（`ui-redesign` 分支）**：深空底色 + 极光渐变光晕背景、玻璃拟态面板、紫蓝渐变主色（#8b7cff → #4d9dff）；侧边栏登录卡渐变描边、账号行选中发光条、指标卡渐变高亮线、表头大写字距排版、弹窗辉光、紫色调细滚动条。
- 功能零变动：仅重写 `index.html` 视觉层，`main.js` 零改动（动态内容全走 CSS 变量自动继承新皮肤），110 个元素 ID 校验零缺失。

## v0.5.35（2026-09-08）— 计费额度全面恢复

- **fix(billing)：统一附加浏览器 User-Agent，绕过官方网关 WAF 拦截，计费三接口恢复可读。**
  逆向定位 + 实测确认根因：此前的 403（code:10085「请求不合法」）**并非 token 权限限制**，而是网关 WAF 将非浏览器 UA 的 Bearer 请求判为脚本流量拦截——单独附加浏览器 UA 即返回 200 与真实额度数据（Origin/Referer 无关）。
- `make_client()` 统一注入 `BROWSER_UA`，一处覆盖全部官方接口请求（计费 / 签到 / 宠物能量全受益）。
- 修正 v0.5.34 的错误结论文案（「daemon 代理 / 桌面 token 无权限」→「WAF UA 拦截」）。
- 更新界面过期接口名标签为「官方计费三接口 summary/paid/free」。

## v0.5.34（2026-09-08）— 计费接口跟进官方改版

- **fix(billing)：改用官方新计费三接口**（#97550 官方将旧单一 `get-user-resource` 拆分）：
  - `get-user-resource-summary`（聚合）
  - `get-user-resource-paid-packages`（付费包，需 `PackageCodes`）
  - `get-user-resource-free-packages`（免费/赠送/体验包，需 `PackageCodes` + 当日切片窗口）
- 内置逆向自官方客户端的 20 个 `COMMODITY_CODES` 真实码值（付费 9 个 / 免费 11 个）。
- `parse_user_resource` 兼容新旧两种响应结构（`data.Accounts` / `data.Response.Data.Accounts`）。
- 对仍被拒的请求返回 `permission_denied` 标记 + 清晰中文说明，消除额度区满屏原始 403；离线有缓存时回退展示并标注。

## v0.5.33（2026-09-08）— 界面脱敏优化

- **fix(ui)：账号列改干净脱敏显示**。此前 `privacy()` 将值拆 pre/mid/suf 三段 `<span>`，手机号等长串显示为割裂的「三段乱码」。
- 新增 `acctLabel`（手机号→`138****7093`、中文名原样、回退脱敏 uid）、`acctUidMask`（→`3913••d3`）、`maskPhone`，替换表格 / 侧边栏 / 详情 / 备份 / 记忆 / 日志的全部账号显示；JWT 与 API Key 保留「小眼睛」逐段模糊。

## v0.5.32（2026-09-08）— 宠物能量 · 盲盒抽奖

- **feat：新增「宠物能量 · 盲盒抽奖」模块**。逆向 growth-center Web 接口落地：
  - `GET /activity/growth/energy`（能量余额）
  - `GET /activity/growth/buddy/quota`（`affordable` 即「已满可抽」判定，能量 ≥ 每抽消耗 10）
  - `POST /activity/growth/buddy/open`（抽盲盒解锁新 buddy）
- 每账号显示能量是否已满，已满一键抽盲盒；接入「刷新全部」自动加载。

## v0.5.30 – v0.5.31（2026-09-02）— macOS 积分修复

- **fix：Mac 登录态路径兼容**（同时支持 `WorkBuddy` / `CodeBuddyExtension` 两种数据目录）+ 扫描兜底 + 诊断探针，修复 macOS 上积分获取失败。
- 修复 `scan_auth_candidates` 类型错误（`PathBuf` → `AsRef<Path>`）。

## v0.5.26 – v0.5.29（2026-09-01 ~ 09-02）— 切换账号置顶会话迁移

- **fix：修复切换账号后置顶会话不迁移**。切换登录态时同步迁移 LevelDB 渲染层的会话置顶状态；兼容新旧 uid 格式；put 重试 + 合并语义修复切号写入失败；修复移动后借用等编译错误。
- 连带修复切换账号闪退、WorkBuddy 不自动重启等问题（v0.5.25）。

## v0.5.22 – v0.5.24（2026-08-31）— 做减法

- **refactor：删除「账号健康池」**（v0.5.22）、**删除「主客户端未运行」提示条**（v0.5.23）——两功能实际收益不达预期，移除以降低复杂度。
- **refactor：切换账号去掉确认框**，一键直接切换（v0.5.24）。

## v0.5.17 – v0.5.21（2026-08-28）— 账号健康池（实验，已移除）

- 账号健康池：积分感知的账号池（`pool_state.json` 落盘 + 禁用/冷却跳过 + 按积分排序推荐）+ 请求级轮转（round-robin 起始位）+ 临近过期刷新（JWT exp 10 分钟偏移标记 needs_refresh 跳过）。
- 用 Windows ToolHelp32 API 检测 WorkBuddy 进程；修复 macOS 交叉编译（cfg-gate）。

## v0.5.10 – v0.5.16（2026-08-28）— 自动化与硬化批次

- **batch B（v0.5.10）**：一键自动签到全部账号（防重入 + 账号间 250ms 限速）。
- **batch C（v0.5.11）**：前端增强——额度趋势图（本地按日去重记录）、预算条、多账号排名、搜索、星标/标签、脱敏导出 MD。
- **batch D（v0.5.12，未单独打 tag）**：安全硬化——单实例锁、签到冷却状态机、token 快照 0o600 权限、只读 SQLite、缓存原子写；v0.5.13 修复 0o600 权限 cfg-gate 导致的跨平台编译失败。
- **batch E/F（v0.5.14–15）**：签到瞬态指数退避重试、签到账号列表去重 + 100 上限防重复触发风暴。
- **v0.5.16**：修复进程检测误报 + `cmd` 弹窗（`CREATE_NO_WINDOW` 隐藏子进程窗口）；撤销自动定时签到；轮询降频至 60s。

## v0.5.6 – v0.5.9（2026-08-27 ~ 08-28）— 多账号额度与用量

- **全部账号官方额度视图**：每账号用各自登录态查官方额度，与本地用量拼成「双视图」（已用多少 vs 还剩多少），覆盖 dashboard 仅当前登录态的局限；支持逐行签到。
- **用量与对话历史**：读取 `session_usage` 消耗明细，支持 JSON/CSV 导出。
- 借鉴官方 dashboard 补齐到期时间轴 + 多账号记忆画像（v0.5.7）；v0.5.8 移除会话防丢工具（功能并入后续版本）。
- **batch A（v0.5.9）**：额度解析健壮性 + 本地缓存 + 重试 + 全中文错误提示；支持 `ALL_PROXY`。

## v0.5.1 – v0.5.5（2026-08-25 ~ 08-26）— 会话管理与置顶迁移

- **切换账号时迁移渲染层会话置顶状态**（LevelDB 直读写）：置顶不因切号丢失；新增单会话加置顶命令。
- 用 rusqlite 内嵌迁移 `workbuddy.db`，修复 Windows 无 sqlite3 CLI 导致的会话搬迁失败（v0.4.1 同源修复）。
- 会话自检/恢复 + 切账号提示面板（v0.5.3）；会话防丢工具：导出 / 置顶诊断 / 悬空清理（v0.5.5）。

## v0.5.0（2026-08-24）— 架构整理

- 多轮逻辑修复与会话搬迁完善；version bump 收口。

## v0.4.0 – v0.4.1（2026-08-24 ~ 08-25）— 数据安全里程碑

- **fix：彻底移除「聚合 ID 合并会话」方案**（v0.4.0）。该方案在 Windows 上改写 `workbuddy.db` 的 `sessions.user_id`，导致多账号会话归属混淆、切换即丢数据。确立设计红线：**切换只换登录态文件，绝不碰会话数据库**。
- 修复切换后 WorkBuddy 不启动（`Command::spawn()` 异步拉起）与双账号昵称串号。
- **fix：用 rusqlite 内嵌迁移 workbuddy.db**（v0.4.1），修复 Windows 会话搬迁失败。

## v0.3.0 – v0.3.1（2026-08-21 ~ 08-24）— 双平台发布

- **feat：多账号批量签到 + 全部账号宠物状态 / 一键派出 / 一键领取**。
- 账号枚举补 `allAccounts` 来源 + 切换即时生成快照（未手动备份过的账号也能直接切）。
- CI 守卫；双平台精确 bundle 目标（Win：NSIS + 便携 zip / Mac：dmg + app）；修复 Windows 便携版打包。
- v0.3.1：清理 README 中已删除的「软件内添加账号」遗留描述。

## v0.2.x 及更早（2026-08 上旬）

- 项目启动：Tauri 2 + Rust 桌面应用雏形；修复多账号切换；CI 自动构建 Win/Mac。
- 宠物自动探险面板上线；修复启动自动加载、派出宠物类型错误等一批早期问题。
- 移除「软件内添加账号」与浏览器登录捕获方案（ reverted，改走「本机已登录账号」路线）。

---

## 图例

- **feat** 新功能 ｜ **fix** 缺陷修复 ｜ **refactor** 重构/移除 ｜ batch A–F 为同日连续迭代的批次命名。
- 未单独打 tag 的版本（v0.5.12 / v0.5.20 / v0.5.30）随相邻 tag 一并发布。
