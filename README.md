# WorkBuddy 账户中枢

> 🌟 **最大亮点：秒切账号，操作极简。** 本机装了多个 WorkBuddy 账号？一键在它们之间切换，切换只换「登录身份」、不动你的任何本地数据，各账号的对话各自独立、完整保留。底层原理见下方 [实现原理](#实现原理为什么切换账号不动你的对话)。

> 一个**本地运行**的跨平台（macOS / Windows）桌面端应用，把 WorkBuddy 账户的日常运维集中到一个窗口：
> **极速多账号切换、多账号计费额度（官方三接口）、每日签到、宠物探险与能量盲盒、登录态管理、AI 记忆画像、API 模型管理（含全局隐私脱敏）**。
> 基于 Tauri 2 + Rust 实现，纯本地数据流，没有云端依赖。

[![GitHub](https://img.shields.io/badge/GitHub-xmgzxmgz%2Fworkbuddy--account--hub-blue?logo=github)](https://github.com/xmgzxmgz/workbuddy-account-hub)
[![Tauri 2](https://img.shields.io/badge/Framework-Tauri%202-blueviolet)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Backend-Rust-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

> ⚠️ **安全声明**：本应用只读取你本机 WorkBuddy 客户端已保存的登录态（`workbuddy-desktop.info`），
> 不会向你索取账号密码，也不会把任何凭证上传到除官方接口以外的第三方。
> **请只在自己信任的机器上运行，并不要把包含登录态的文件提交到任何仓库。**

---

## 🌟 核心优势

**一句话：在多账号之间切换，你的对话历史永远不会丢；所有账号的积分额度，一个窗口全看清。**

很多人同时用多个 WorkBuddy 账号（工作号 / 小号 / 测试号），但官方客户端一次只能登一个，切来切去最怕「之前的对话不见了」，更怕「每个号还剩多少额度要挨个登录去查」。WorkBuddy 账户中枢从架构层面解决了这两件事：

- ⚡ **秒级切换**：侧边栏列出本机所有登录过的账号（含从未手动备份过的），点一下即切换，自动重启 WorkBuddy 生效。
- 💾 **本地数据零改动**：切换只换「登录身份」，绝不改写 WorkBuddy 的会话数据库。每个账号的对话按 uid 隔离、各自完整保留。
- 📊 **多账号额度全景**：每个账号用各自的登录态查官方计费接口，赠送包 / 体验版 / 全部剩余、到期时间、使用率一屏尽览——官方 dashboard 只能看到当前登录的一个号。
- 🛡 **自动兜底备份**：每次切换前，中枢会把「即将离开的账号」整份登录态 + local_storage 自动备份进本地保险库，极端情况也能还原。
- 🔁 **任意往返**：可切换账号列表只增不减，多个账号随便切、随便回，没有「切出去就回不来」。

想知道底层是怎么做到的？看下面的 [实现原理](#实现原理为什么切换账号不动你的对话)。

---

## ✨ 功能一览

| 模块 | 能力 | 状态 |
| --- | --- | --- |
| **🌟 极速多账号切换** | 列出本机所有登录过的账号，一键切换且**对话历史 100% 保留**（仅换登录态，含置顶状态迁移） | ✅ 稳定 |
| **📊 多账号额度全景** | 每账号独立查官方计费三接口，赠送包/体验版/全部剩余、最早到期、使用率，支持排名/搜索/脱敏导出 MD | ✅ 稳定 |
| 概览卡片 | 权益赠送包剩余 / 体验版剩余 / 全部剩余 / 登录态剩余天数 | ✅ 稳定 |
| 账户档案 | 昵称、UID、UIN、账号类型、手机号、最近登录、管理员（全列干净脱敏） | ✅ 稳定 |
| 登录态有效期 | 解析 JWT，签发者 / 过期时间 / 剩余天数（圆环进度） | ✅ 稳定 |
| 积分套餐明细 | 官方计费三接口完整解析，套餐明细 + 额度趋势图（本地按日记录） | ✅ 稳定 |
| **每日签到** | 官方签到接口，连续天数 / 最近记录；单账号 + **全部账号一键批量签到** | ✅ 稳定 |
| **宠物自动探险** | 派出宠物 → 显示派出状态/地点 → 归来自动领取积分，日志可见 | ✅ 稳定 |
| **🐾 宠物能量 · 盲盒抽奖** | 各账号能量是否已满一目了然，已满一键抽盲盒解锁新 buddy | ✅ 新增 |
| AI 记忆画像 | 按 `## 标题` 折叠展示，整段隐私默认高斯模糊 | ✅ 稳定 |
| 用量与对话历史 | 本地 `session_usage` 消耗明细，JSON / CSV 导出 | ✅ 稳定 |
| 本机环境 | 客户端版本、构建号、安装大小、平台 / 架构 | ✅ 稳定 |
| API 模型管理 | 自定义 key 增删改测 / 官方渠道 / 当前使用探测 / 重启生效 | ✅ 稳定 |
| **全局隐私脱敏** | 顶栏 👁 一键控制所有隐私字段（Key/UID/手机号/昵称/JWT/记忆画像/模型地址）中段模糊 | ✅ 稳定 |

> 💡 **双视图理念**：上方「用量与对话历史」是本地实际消耗（已用多少 token/积分）；「多账号额度」是官方剩余额度（还剩多少/何时到期）。两者拼成完整用量视图，且覆盖全部已登记账号。

---

## 预览

**主仪表盘**（积分 / 套餐到期 / 登录态有效期 / 记忆画像）

![主仪表盘](docs/images/dashboard.png)

**API 管理面板**（自定义 / 官方渠道 / 当前使用 + 全局隐私小眼睛）

![API 管理](docs/images/api-manager.png)

**宠物自动探险面板**（派出状态 / 地点 / 倒计时 / 日志）

![宠物自动探险](docs/images/buddy-travel.png)

---

## 📦 安装

macOS 需要 Xcode Command Line Tools（`xcode-select --install`）；Windows 需要 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)（通常已预装）。

### 直接下载（推荐）

从 [Releases](https://github.com/xmgzxmgz/workbuddy-account-hub/releases) 下载最新版：
- **Windows**：`WorkBuddy-Account-Hub_<版本>_portable-x64.zip`（绿色便携，解压即用）或 `WorkBuddy.Account.Hub_<版本>_x64-setup.exe`（安装版）
- **macOS**：`WorkBuddy.Account.Hub_<版本>_aarch64.dmg` / `.app.tar.gz`（Apple Silicon）

### 从源码构建

```bash
# 1. 克隆
git clone https://github.com/xmgzxmgz/workbuddy-account-hub.git
cd workbuddy-account-hub

# 2. 安装前端依赖（仅用于 tauri build）
cd tauri-app
npm install

# 3. 打包（首次下载 Rust crate 并编译，macOS / Windows 通用）
npx tauri build
# 产物：tauri-app/src-tauri/target/release/bundle/...
#   macOS → bundle/macos/*.app / *.dmg；Windows → bundle/nsis/*.exe
```

---

## 🚀 快速上手

### 日常流程
1. 打开 App → 顶部「刷新全部」自动读本机登录态 + 拉取全部数据
2. 「仅查积分」/「一键签到」按需触发单独请求
3. 「导出报告」生成可粘贴的 Markdown 快照，方便备份与排错

### 多账号额度全景
1. 打开「全部账号额度」面板 → 点「查询全部额度」
2. 每个账号用各自的登录态快照独立查询，互不影响当前登录
3. 支持按剩余排名、按昵称/UID 搜索、⬇ 导出脱敏 Markdown

### 宠物：探险与能量盲盒
1. 打开「宠物」面板，看到当前状态：在窝休息 / 正在前往 / 已到达
2. 地点列表**点击即可派出**宠物 → 倒计时显示归来时间；归来后**自动领取积分**（面板每 30 秒自动轮询）
3. 「宠物能量」面板显示每个账号的能量是否已满，已满可**一键抽盲盒**解锁新 buddy

### API 模型管理
1. 在「自定义 API」区管理 `~/.workbuddy/models.json`：新增 / 编辑 / 删除 / 一键测试连接
2. 改完点顶部「重启 WorkBuddy」让配置生效
3. 当前主程序正在使用哪个模型由顶栏探测条实时显示

### 全局隐私脱敏
- 默认状态：所有隐私字段**只露首尾 + 中段模糊**
- 点顶栏 👁 隐私 → 全部清晰（图标变 🙈），再点 → 恢复模糊

---

## 🔧 从源码构建（开发 / 二次开发）

> 本仓库为 Tauri 2 桌面应用，修改 Rust 或前端后需重新构建才能生效（前端资源内嵌 exe）。

### 前置要求
- Rust 工具链：`rustup` + `cargo`（<https://rustup.rs>）
- Node.js ≥ 18（仅前端依赖）
- Windows：系统 WebView2（通常已自带）；macOS：Xcode Command Line Tools

### 构建 / 运行
```bash
cd tauri-app
npm install            # 安装前端依赖（如需）
npm run tauri dev      # 开发模式（热重载前端，Rust 改动自动重编）
# 或打出安装包 / 便携版
npm run tauri build
```

### 双平台自动构建（macOS + Windows）

仓库已配置 GitHub Actions（`.github/workflows/release.yml`），**推送 `v*` tag 即自动构建并发布双平台产物**，无需本地准备 macOS 环境：

```bash
git tag v0.5.36
git push origin main --tags
# → Actions 自动跑 build-macos (dmg/app) + build-windows (nsis + 便携 zip)
# → 产物自动发布到 Releases
```

> macOS 产物未签名，Apple Silicon 上若提示「无法验证开发者」，需在「系统设置 → 隐私与安全性」中手动允许。
> 也可在 GitHub 仓库 **Actions → Release Win / Mac → Run workflow** 手动触发。

---

## 🔧 实现原理：为什么切换账号不动你的对话

很多「多账号切换」工具的实现是「整体覆盖用户数据目录」——切到 B 就把 A 的数据目录换掉，于是 A 的对话就丢了。本项目的做法正好相反，核心只有一句话：

> **切换账号 = 只换登录态文件，绝不碰会话数据库。**

下面逐层拆解：

### 1. WorkBuddy 的会话存在哪里，怎么和账号绑定

WorkBuddy 把每个账号的对话历史存在本机一个**会话数据库**里（`~/.workbuddy/workbuddy.db` 的 `sessions` 表）。
关键在于：**每条会话都带有自己的账号 uid**（即登录态 token 里的 `sub`）。WorkBuddy 启动后以当前登录账号的 uid 查询，只显示「当前登录账号」名下的会话——各账号的对话天然按 uid 隔离。

### 2. 中枢切换时到底改了什么

切换命令 `switch_account` 只做一件事：重写本机登录态文件 `workbuddy-desktop.info` 里的 `account` / `allAccounts` 字段，把「当前账号」指向你要切过去的那一个（带上它自己的真实 token）。

它**完全不读写 `workbuddy.db`**——那个装着你全部对话的会话数据库从头到尾原样保留。

于是：
- 切到 B 账号 → 登录态变成 B（带 B 的真实 token），WorkBuddy 启动时只展示 B 名下的对话；A 的对话静静躺在数据库里，一行没动。
- 切回 A 账号 → 登录态变回 A，A 的对话立刻全部回来。

这就是「切 100 次也不丢」的根本原因：**会话数据从来没被移动、覆盖或改写过，只是「当前视角」在账号之间平移。**

### 3. 置顶状态跟着账号走

切换登录态时，中枢会同步迁移渲染层（LevelDB）里按 uid 归属的**会话置顶状态**，兼容新旧 uid 格式——各账号的置顶布局互不串扰（v0.5.26–29 一系列修复的成果）。

### 4. 切走之前先自动备份（兜底）

为了防住极端情况，中枢在每次切换前，会先把「即将离开的账号」整份登录态 + `local_storage` 自动备份进本地保险库 `vault/<uid>/history/<时间戳>/`。
切不回来？从历史备份里还原即可，对话零损失。

### 5. 可切换账号列表「只增不减」+ 未备份账号也能切

切换时 `allAccounts` 用**合并**而非覆盖；且对未手动快照过的账号，会在 `allAccounts` 里找到该账号时**即时生成快照**——任何在本机登录过的账号都能直接一键切换。

### 小结

| 维度 | 常见多账号工具 | 本账户中枢 |
| --- | --- | --- |
| 切换时是否动会话数据 | 整体覆盖 → 易丢 | 只换登录态 → 不动 |
| 切回原账号 | 可能丢失历史 | 历史原样保留 |
| 多账号对话 | 合并易损坏 | 按 uid 隔离，各自完整 |
| 置顶状态 | 丢失/串号 | 随账号迁移 |
| 兜底 | 无 | 每次切换前自动备份 |

**结论**：无论你本机有 2 个还是 100 个账号，切换只平移「当前身份」，会话仓库始终不动——各账号对话独立、安全、可随时切回。

---

## 📡 接口说明

本应用仅调用 WorkBuddy **官方接口**（`copilot.tencent.com`），不连任何第三方服务。所有请求统一携带浏览器 User-Agent（官方网关会拦截非浏览器 UA 的 API 请求，返回 403「请求不合法」——这是本应用踩过并修复的坑，见 [CHANGELOG v0.5.35](CHANGELOG.md)）。

| 用途 | 方法 | 路径 |
| --- | --- | --- |
| 计费聚合 | POST | `/billing/meter/get-user-resource-summary` |
| 付费套餐明细 | POST | `/billing/meter/get-user-resource-paid-packages` |
| 免费/赠送/体验包明细 | POST | `/billing/meter/get-user-resource-free-packages` |
| 签到活动状态 | POST | `/billing/meter/checkin-activity-status` |
| 执行签到 | POST | `/billing/meter/daily-checkin` |
| 宠物能量余额 | GET | `/activity/growth/energy` |
| 宠物盲盒配额 | GET | `/activity/growth/buddy/quota` |
| 抽盲盒 | POST | `/activity/growth/buddy/open` |
| 宠物旅行状态/配置/派出/领取 | GET/POST | `/activity/growth/buddy/travel/*` |

> 认证方式：`Authorization: Bearer <accessToken>` + `X-User-Id: <uid>`（与本机 WorkBuddy 客户端同一套登录态）。

---

## 🛠 技术架构

```
workbuddy-account-hub/
├── tauri-app/                  # Tauri 桌面应用（macOS / Windows）
│   ├── src/                    # 前端（HTML + JS，无框架，原生 WebView，内嵌 exe）
│   │   ├── index.html          # 单页 UI（侧边栏 + 仪表盘 + 多账号面板）
│   │   └── main.js             # 渲染 / 交互 / 轮询 / 全局隐私 / 脱敏
│   └── src-tauri/              # Rust workspace
│       ├── crates/wb_api/      # 官方接口封装（计费三接口/签到/能量盲盒/宠物旅行/模型/环境）
│       ├── crates/account_ops/ # 登录态扫描 / 快照 / 切换 / 置顶迁移
│       ├── src/main.rs         # Tauri command 注册
│       └── tauri.conf.json
├── sync/                       # 多设备对话同步（极空间 / Tailscale）
├── vault/                      # 本地数据快照（敏感，不提交）
├── vault.py / detect.py / switch.py  # 配套运维脚本
└── docs/images/                # README 截图
```

- **前端**：原生 JS + Tauri WebView，无 React/Vue 依赖（够用）
- **后端**：Rust workspace（`wb_api` 接口层 + `account_ops` 账户操作层）+ reqwest blocking + rustls-tls
- **打包**：`tauri build` + GitHub Actions 自动出 Win / Mac 产物
- **数据源**：本机登录态 + `~/.workbuddy/models.json` + 本地 `workbuddy.db`（只读）
- **安全**：切换/快照/读写均在本地完成，不主动上传任何凭证

---

## 🔒 安全与隐私

- **本地优先**：所有数据读取与接口调用都发生在你本机
- **不打包凭证**：仓库 `.gitignore` 忽略 `*.info`、`vault/` 等可能含登录态的文件
- **脱敏显示**：UI 默认所有隐私字段中段模糊，点眼睛才完整；账号列始终干净脱敏
- **登录态过期**：JWT 有效期以官方签发为准（当前约 60 天），失效后重新登录 WorkBuddy 客户端即可

---

## ❓ 常见问题

- **登录态失效（401）**：Token 过期或被踢下线，重新登录 WorkBuddy 客户端即可，App 自动读取新登录态。
- **额度显示「被网关拒绝」**：官方网关会拦截非浏览器 UA 的请求。v0.5.35 起已统一附加浏览器 UA 正常出数；若个别账号仍被拒，多为该账号自身权限问题。
- **额度数据与官方 dashboard 不一致**：官方 dashboard 只显示当前登录账号；本应用是每账号独立查询，数据更全。以官方页面为准可随时交叉验证。
- **宠物一直在「旅行中」**：派出的宠物需数小时才归来，面板会自动轮询，归来即自动领取，属正常现象。
- **API 改了不生效**：改完 `models.json` 后点顶部「重启 WorkBuddy」让配置生效。
- **Windows 路径问题**：若安装包在 Windows 上找不到登录态，请确认已用最新 Release（已做跨平台路径适配）。

---

## 📦 Releases

每次发布自动构建并附带：
- **Windows**（x64）：便携版 `.zip`（解压即用）+ 安装版 `.exe`（NSIS）
- **macOS**（Apple Silicon）：`.dmg` + `.app.tar.gz`

前往 [Releases 页](https://github.com/xmgzxmgz/workbuddy-account-hub/releases) 查看。

---

## 📝 更新日志

完整版本历史见 [CHANGELOG.md](CHANGELOG.md)（v0.2.x → v0.5.35 全量记录）。

---

## 🙏 致谢

宠物自动探险功能的接口协议，与 [workbuddy-checkin-qinglong](https://github.com/xmgzxmgz/workbuddy-checkin-qinglong)
项目同步维护，感谢该领域社区用户的需求推动。

---

## 相关项目

- [workbuddy-checkin-qinglong](https://github.com/xmgzxmgz/workbuddy-checkin-qinglong) — 青龙面板版自动签到 + 宠物自动探险（纯 Python）
- [workbuddy-account-dashboard](https://github.com/xmgzxmgz/workbuddy-account-dashboard) — 同功能的网页版仪表盘

---

## 许可

MIT
