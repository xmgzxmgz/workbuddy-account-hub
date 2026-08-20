# WorkBuddy 账户中枢

> 一个**本地运行**的 macOS 桌面端应用，把 WorkBuddy / CodeBuddy 账户的日常运维集中到一个窗口：
> **积分可视化、每日签到、宠物自动探险、登录态管理、AI 记忆画像、API 模型管理（含全局隐私中段模糊）**。
> 基于 Tauri 2 + Rust 实现，纯本地数据流，没有云端依赖。

[![GitHub](https://img.shields.io/badge/GitHub-xmgzxmgz%2Fworkbuddy--account--hub-blue?logo=github)](https://github.com/xmgzxmgz/workbuddy-account-hub)
[![Tauri 2](https://img.shields.io/badge/Framework-Tauri%202-blueviolet)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Backend-Rust-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

> ⚠️ **安全声明**：本应用只读取你本机 WorkBuddy 客户端已保存的登录态（`~/Library/Application Support/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info`），
> 不会向你索取账号密码，也不会把任何凭证上传到除官方接口以外的第三方。
> **请只在自己信任的机器上运行，并不要把包含登录态的文件提交到任何仓库。**

---

## ✨ 功能一览

| 模块 | 能力 | 状态 |
| --- | --- | --- |
| 概览卡片 | 权益赠送包剩余 / 体验版剩余 / 全部剩余 / 登录态剩余天数 | ✅ 稳定 |
| 账户档案 | 昵称、UID、UIN、账号类型、手机号、最近登录、管理员 | ✅ 稳定 |
| 登录态有效期 | 解析 JWT，签发者 / 过期时间 / 剩余天数（圆环进度） | ✅ 稳定 |
| 积分套餐明细 | 官方 `get-user-resource` 完整解析，到期时间轴（30 天内红色预警） | ✅ 稳定 |
| **每日签到** | 官方签到接口，连续天数 / 最近记录，一键签到领积分 | ✅ 稳定 |
| **宠物自动探险** | 派出宠物 → 显示派出状态/地点 → 归来自动领取积分，日志可见 | ✅ 新增 |
| AI 记忆画像 | 按 `## 标题` 折叠展示，整段隐私默认高斯模糊 | ✅ 稳定 |
| 本机环境 | 客户端版本、构建号、安装大小、平台 / 架构 | ✅ 稳定 |
| API 模型管理 | 自定义 key 增删改测 / 官方渠道 / 当前使用探测 / 重启生效 | ✅ 稳定 |
| **全局隐私小眼睛** | 顶栏 👁 一键控制所有隐私字段（Key/UID/手机号/昵称/JWT/记忆画像/模型地址）中段模糊 | ✅ 稳定 |

> 💡 **宠物自动探险**是本版新增亮点：复用 WorkBuddy「成长中心」的宠物旅行玩法，
> 在桌面端可视化展示宠物状态（在窝休息 🏠 / 正在前往某地 / 已到达可领奖），
> 归来后**自动领取积分**，并记录完整旅行日志。每天限派 1 次，旅行通常 1~4 小时。

---

## 📦 安装

需要 macOS + Xcode Command Line Tools（`xcode-select --install`）。

### 直接下载（推荐）

从 [Releases](https://github.com/xmgzxmgz/workbuddy-account-hub/releases) 下载最新版：
- **macOS**：`WorkBuddy.Account.Hub_<版本>_aarch64.dmg` / `.app.tar.gz`
- **Windows**：`WorkBuddy.Account.Hub_<版本>_x64-setup.exe` / `.msi`

双击挂载 / 运行安装器即可。

### 从源码构建

```bash
# 1. 克隆
git clone https://github.com/xmgzxmgz/workbuddy-account-hub.git
cd workbuddy-account-hub

# 2. 安装前端依赖（仅用于 tauri build）
cd tauri-app
npm install

# 3. 打包（首次下载 Rust crate 并编译）
./node_modules/.bin/tauri build
# 产物：tauri-app/src-tauri/target/release/bundle/...

# 4. 安装到 /Applications（macOS）
cp -R "tauri-app/src-tauri/target/release/bundle/macos/WorkBuddy Account Hub.app" /Applications/
open "/Applications/WorkBuddy Account Hub.app"
```

---

## 🚀 快速上手

### 日常流程
1. 打开 App → 顶部「刷新全部」自动读本机登录态 + 拉取全部数据
2. 「仅查积分」/「一键签到」按需触发单独请求
3. 「导出报告」生成可粘贴的 Markdown 快照，方便备份与排错

### 宠物自动探险
1. 打开「宠物」面板，看到当前状态：在窝休息 / 正在前往 / 已到达
2. 地点列表（咖啡馆等）**点击即可派出**宠物 → 倒计时显示归来时间
3. 归来后**自动领取积分**，奖励写入面板 + 日志自动记录派发/归来/领取全过程
4. 面板内置**自动轮询**（每 30 秒），无需手动刷新

### API 模型管理
1. 在「自定义 API」区管理 `~/.workbuddy/models.json`：新增 / 编辑 / 删除 / 一键测试连接
2. 改完点顶部「重启 WorkBuddy」让配置生效
3. 当前主程序正在使用哪个模型由顶栏探测条实时显示

### 全局隐私眼睛
- 默认状态：所有隐私字段**只露首尾 + 中段高斯模糊**
- 点顶栏 👁 隐私 → 全部清晰（图标变 🙈），再点 → 恢复模糊

---

## 📡 接口说明

本应用仅调用 WorkBuddy **官方接口**（`copilot.tencent.com`），不连任何第三方服务。

| 用途 | 方法 | 路径 |
| --- | --- | --- |
| 积分套餐 | POST | `/v2/billing/meter/get-user-resource` |
| 签到状态 | GET | `/v2/billing/meter/checkin-activity-status` |
| 执行签到 | POST | `/v2/billing/meter/daily-checkin` |
| 宠物旅行状态 | GET | `/activity/growth/buddy/travel/status` |
| 宠物旅行配置 | GET | `/activity/growth/buddy/travel/config` |
| 宠物派出 | POST | `/activity/growth/buddy/travel/depart` |
| 宠物积分领取 | POST | `/activity/growth/buddy/travel/claim` |

---

## 🛠 技术架构

```
workbuddy-account-hub/
├── tauri-app/                  # Tauri 桌面应用（macOS / Windows）
│   ├── src/                    # 前端（HTML + JS，无框架，原生 WebView）
│   │   ├── index.html          # 单页 UI（侧边栏 + 仪表盘 + 宠物面板 + API 管理）
│   │   └── main.js             # 渲染 / 交互 / 宠物轮询 / 全局隐私
│   └── src-tauri/              # Rust 后端
│       ├── crates/wb_api/      # 登录态 / 积分 / 签到 / 宠物 / 模型 / 环境
│       ├── src/main.rs         # Tauri command 注册
│       └── tauri.conf.json
├── sync/                       # 多设备对话同步（极空间 / Tailscale）
├── vault/                      # 本地数据快照（敏感，不提交）
├── vault.py / detect.py / switch.py  # 配套运维脚本
└── docs/images/                # README 截图
```

- **前端**：原生 JS + Tauri WebView，无 React/Vue 依赖（够用）
- **后端**：Rust + reqwest blocking + rustls-tls（直连官方接口，无 Node 桥）
- **打包**：`tauri build` + GitHub Actions 自动出 Win / Mac 安装包
- **数据源**：本机登录态 + `~/.workbuddy/models.json`
- **安全**：切换/快照/读写均在本地完成，不主动上传任何凭证

---

## 🔒 安全与隐私

- **本地优先**：所有数据读取与接口调用都发生在你本机
- **不打包凭证**：仓库 `.gitignore` 忽略 `*.info`、`vault/` 等可能含登录态的文件
- **脱敏显示**：UI 默认所有隐私字段中段高斯模糊，点眼睛才完整
- **登录态过期**：JWT 通常约 90 天有效，失效后重新登录 WorkBuddy 客户端即可

---

## ❓ 常见问题

- **登录态失效（401）**：Token 过期或被踢下线，重新登录 WorkBuddy 客户端即可，App 自动读取新登录态。
- **宠物一直在「旅行中」**：派出的宠物需数小时才归来，面板会自动轮询，归来即自动领取，属正常现象。
- **API 改了不生效**：改完 `models.json` 后点顶部「重启 WorkBuddy」让配置生效。
- **Windows 路径问题**：若安装包在 Windows 上找不到登录态，请确认已用最新 Release（已做跨平台路径适配）。

---

## 📦 Releases

每次发布自动构建并附带：
- **macOS**（Apple Silicon）：`.dmg` + `.app.tar.gz`
- **Windows**（x64）：`.exe`(NSIS) + `.msi`

前往 [Releases 页](https://github.com/xmgzxmgz/workbuddy-account-hub/releases) 查看。

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
