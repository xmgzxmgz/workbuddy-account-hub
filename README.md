# WorkBuddy 账户中枢

一个**本地运行**的 macOS 桌面端应用，把 WorkBuddy / CodeBuddy 账户的日常运维集中到一个窗口：
**积分额度可视化、签到、登录态快照、AI 记忆画像、API 模型管理（含隐私中段模糊）**。
基于 Tauri 2 + Rust 实现，纯本地数据流，没有云端依赖。

> ⚠️ 本应用只读取你本机 WorkBuddy 客户端已保存的登录态（位于
> `~/Library/Application Support/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info`），
> 不会向你索取账号密码，也不会把任何凭证上传到除官方接口以外的第三方。
> **请只在自己信任的机器上运行，并不要把包含登录态的文件提交到任何仓库。**

---

## 预览

**主仪表盘**（积分 / 套餐到期 / 登录态有效期 / 记忆画像）

![主仪表盘](docs/images/dashboard.png)

**API 管理面板**（自定义 / 官方渠道 / 当前使用 + 全局隐私小眼睛）

![API 管理](docs/images/api-manager.png)

---

## 功能

- **概览卡片**：权益赠送包剩余 / 体验版剩余 / 全部剩余 / 登录态剩余天数，一眼看清资源
- **账户档案**：昵称、UID、UIN、账号类型、手机号、最近登录、管理员
- **登录态有效期**：解析 JWT，显示签发者 / 过期时间 / 剩余天数（圆环进度）
- **积分套餐明细**：官方 `/billing/meter/get-user-resource` 完整解析，
  按「体验版 vs 赠送包」分类，附**到期时间轴**（30 天内红色预警）
- **每日签到**：官方 `/billing/meter/checkin-activity-status`，连续天数、最近记录一目了然
- **AI 记忆画像**：按 `## 标题` 折叠展示，整段隐私默认高斯模糊
- **本机环境**：客户端版本、构建号、安装大小、平台 / 架构
- **API 管理面板**（独家）
  - 自定义 API：读 `~/.workbuddy/models.json`，新增 / 编辑 / 删除 / 一键测试连接
  - 官方渠道 API：内置 7 个模型只读展示
  - 当前使用：实时探测 WorkBuddy 在用模型
  - 「重启 WorkBuddy」按钮：让 models.json 改动生效
- **全局隐私小眼睛**：顶栏唯一一个 👁 隐私 按钮，控制所有隐私字段
  （Key / UID / 手机号 / 昵称 / JWT / 记忆画像 / 模型地址）中段的高斯模糊与清晰显示

---

## 快速开始（macOS）

需要 macOS + Xcode Command Line Tools（`xcode-select --install`）。

### 直接安装（推荐）

从 [Releases](https://github.com/xmgzxmgz/workbuddy-account-hub/releases) 下载 `WorkBuddy Account Hub_*.dmg`，
双击挂载，把 `WorkBuddy Account Hub.app` 拖入 `/Applications` 即可。

### 从源码构建

```bash
# 1. 克隆
git clone https://github.com/xmgzxmgz/workbuddy-account-hub.git
cd workbuddy-account-hub

# 2. 安装前端依赖（仅用于 tauri build）
cd tauri-app
npm install

# 3. 打包（首次会下载 Rust crate 并编译，约 1-2 分钟）
./node_modules/.bin/tauri build
# 产物：
#   tauri-app/src-tauri/target/release/bundle/macos/WorkBuddy Account Hub.app
#   tauri-app/src-tauri/target/release/bundle/dmg/WorkBuddy Account Hub_*.dmg

# 4. 安装到 /Applications
cp -R "tauri-app/src-tauri/target/release/bundle/macos/WorkBuddy Account Hub.app" /Applications/
open "/Applications/WorkBuddy Account Hub.app"
```

> Windows / Linux 用户：理论上 Tauri 2 跨平台可用，但本仓库的 macOS 路径硬编码，
> 需要把 `authCandidates()` 等本地路径改成对应平台。

---

## 使用说明

### 日常流程
1. 打开 App → 顶部「刷新全部」自动读本机登录态 + 拉取全部数据
2. 「仅查积分」/「一键签到」按需触发单独请求
3. 「导出报告」生成可粘贴的 Markdown 快照，方便备份与排错

### API 模型管理
1. 在「自定义 API」区管理 `~/.workbuddy/models.json`：
   - 新增：填名称 / id / url / key / provider / 能力勾选
   - 编辑：保留 id，key 留空则保留原值
   - 测试：真实 POST `/chat/completions` 测延迟与响应模型
2. 改完点顶部「重启 WorkBuddy」让配置生效
3. 当前主程序正在使用哪个模型由顶栏探测条实时显示（探测自本机 leveldb）

### 全局隐私眼睛
- 默认状态：所有隐私字段**只露首尾 + 中段高斯模糊**
- 点顶栏 👁 隐私 → 全部清晰（图标变 🙈），再点 → 恢复模糊
- 覆盖字段：API Key / UID / 手机号 / 昵称 / 记忆画像 / 模型请求地址 / JWT 主题与受众等

---

## 技术架构

```
workbuddy-account-hub/
├── tauri-app/                  # Tauri 桌面应用（macOS）
│   ├── src/                    # 前端（HTML + JS，无框架，原生 WebView）
│   │   ├── index.html          # 单页 UI（侧边栏 + 仪表盘 + API 管理）
│   │   └── main.js             # 渲染 / 交互 / 全局隐私 privacy()
│   └── src-tauri/              # Rust 后端
│       ├── crates/wb_api/      # 登录态 / 积分 / 模型 / 环境
│       ├── src/main.rs         # Tauri command 注册
│       └── tauri.conf.json
├── credits/                     # 共享的 Node 端积分 / 签到脚本（credits-api.js / debug-server.mjs / web.html）
├── publish-checkin-qinglong/   # 发布到 workbuddy-checkin-qinglong 仓库的同步脚本
├── publish-dashboard/          # 发布到 workbuddy-account-dashboard 仓库的同步脚本
├── sync/                       # 多设备对话同步（极空间 / Tailscale）
├── vault/                      # 本地数据快照（敏感，不提交）
├── vault.py                    # 快照脚本
├── detect.py                   # 本机环境探测
├── switch.py                   # 账号切换辅助脚本
└── docs/images/                # README 截图
```

- **前端**：原生 JS + Tauri WebView，无 React/Vue 依赖（够用）
- **后端**：Rust + reqwest 0.12 blocking + rustls-tls（直连官方接口，无 Node 桥）
- **打包**：`tauri build` 输出 `.app` + `.dmg`
- **数据源**：本机登录态 + `~/.workbuddy/models.json`（WorkBuddy 官方读取文件）
- **切换模型机制**：改 `models.json` + 重启 WorkBuddy（不改 WorkBuddy 运行内存，安全）

---

## 安全与隐私

- **本地优先**：所有数据读取与接口调用都发生在你本机，应用不发起任何外部网络连接
- **不打包凭证**：仓库 `.gitignore` 忽略 `*.info`、`vault/` 等可能含登录态的文件
- **脱敏显示**：UI 默认对所有隐私字段做中段模糊（`filter: blur(5px)`），点眼睛才显示完整
- **API Key 完整值**只在点眼睛时渲染到 DOM，未点眼睛走的是掩码串（前 6 后 4）
- **登录态会过期**：JWT 通常约 90 天有效，失效后需重新登录 WorkBuddy 客户端

---

## 已知限制

- **平台**：默认路径针对 macOS 桌面端；Windows / Linux 用户需修改 `authCandidates()` 等路径
- **API 切换**：本应用管理的是 `~/.workbuddy/models.json`，具体当前使用哪个模型由你在 WorkBuddy 主程序内点选；本应用只负责配置生效，不强行覆盖
- **后台登录态探测**：当前使用模型通过扫描 `~/.workbuddy/app/session/Local Storage/leveldb` 获取，准确率与 WorkBuddy 写入时机相关

---

## 相关项目

- [workbuddy-account-dashboard](https://github.com/xmgzxmgz/workbuddy-account-dashboard) — 同功能的网页版（Node 静态服务）
- [workbuddy-checkin-qinglong](https://github.com/xmgzxmgz/workbuddy-checkin-qinglong) — 青龙面板版自动签到

---

## 许可

MIT