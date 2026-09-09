# 更新日志（Changelog）

所有版本均按语义化演进记录；日期为对应 tag 的提交日期。
格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，倒序排列。

> 💡 每次发布由 GitHub Actions 自动构建双平台产物（macOS `.dmg`/`.app.tar.gz`，Windows `.exe`(NSIS) + 便携版 `.zip`），见 [Releases](https://github.com/xmgzxmgz/workbuddy-account-hub/releases)。

---

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
