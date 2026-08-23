# Alter 1.1.0 前端简体中文化执行任务

## 背景

当前本机安装的是 Alter 1.1.0。官方 v1.1.0 前端没有国际化框架，用户可见英文直接写在 React/TypeScript 源码中。用户明确要求修改前端源码实现真正中文化，不使用浏览器翻译。

## 目标

将 Alter Web Dashboard 的用户可见界面完整翻译为简体中文，并保持所有进程管理、认证、日志、终端、定时任务、隧道、通知和设置功能不变。

## 允许修改范围

- `web-ui/index.html`
- `web-ui/src/**/*.ts`
- `web-ui/src/**/*.tsx`
- 为中文化新增的前端测试文件（仅限 `web-ui/src/**`）

## 禁止修改范围

- `src/**` Rust 后端代码
- `Cargo.toml`、`Cargo.lock`、前端依赖清单和锁文件
- API 协议、路由、请求/响应字段、内部枚举值
- 进程管理行为、认证状态、密码、用户数据及 `%APPDATA%\alter-pm2`
- 构建产物 `web-ui/dist/**`（由构建命令生成，不手工编辑）
- 项目外任何文件
- Git 提交、推送、历史重写、发布和部署

## 已确定实现要求

1. 直接修改前端源码中的用户可见字符串，覆盖导航、标题、表格、按钮、菜单、对话框、提示、通知、空状态、表单标签、占位符、校验错误、登录页、设置页、终端面板与 AI 面板。
2. `web-ui/index.html` 的 `lang` 改为 `zh-CN`，页面标题改为中文但保留 Alter 产品名。
3. 不翻译或改写 API 字段、内部状态值、命令、文件路径、URL、环境变量名、代码示例。
4. 保留产品名与常用技术缩写，例如 Alter、Cloudflare、ngrok、GitHub、Ollama、Telegram、CPU、GPU、PID、SSH、RAM、VRAM、JSON、URL。
5. 状态值仅在显示层翻译，逻辑判断仍使用原始英文枚举。
6. 不引入新的 npm 依赖或完整 i18n 框架；本次采用清晰、可审查的中文字符串替换。
7. 中文术语应一致，例如：Processes=进程、Cron Jobs=定时任务、Log Library=日志库、Log Volume=日志量、Port Finder=端口查找、Tunnels=隧道、Settings=设置、Restart=重启、Stop=停止、Delete=删除、Save state=保存状态、Shutdown daemon=关闭守护进程。
8. 不翻译用户数据、项目名、进程名、日志正文、命令输出和模型返回内容。
9. 不读取或记录任何密码、Token、Cookie、`.env` 内容或浏览器数据。

## 验收标准

- 登录页、主导航、进程列表及详情、启动/编辑进程、定时任务、日志库、日志量、端口查找、隧道、通知、设置、终端、AI 助手的静态界面均为简体中文。
- 所有交互控件的 `title`、`aria-label`、`placeholder`、确认/取消按钮和成功/错误提示完成中文化。
- API 字段、命令、内部枚举、技术缩写与产品名未被破坏。
- TypeScript 构建、测试、lint 和格式检查通过，或明确报告仓库原有失败及证据。
- `rg` 复核后不存在明显遗漏的用户可见英文；无法翻译的技术词须列入返回说明。
- 修改范围仅限允许文件和本任务文件。

## 测试命令

在 `web-ui` 目录运行：

```powershell
npm ci
npm run format:check
npm run lint
npm test
npm run build
```

在仓库根目录进行只读范围检查：

```powershell
git status --short
git diff --stat
git diff --check
rg -n 'Processes|Cron Jobs|Log Library|Port Finder|Settings|Start new process|Save state|Shutdown daemon' web-ui/src web-ui/index.html
```

## 返回格式

- `status`: success / partial / blocked
- `changedFiles`: 实际修改文件清单
- `implementationSummary`: 翻译覆盖范围与关键实现说明
- `testSummary`: 每个测试命令及结果
- `remainingEnglish`: 有意保留或尚未处理的英文及原因
- `risks`: 风险与兼容性说明
- `reviewTargets`: 请 Codex 重点复核的位置
- `blockers`: 阻塞项；无则写“无”
