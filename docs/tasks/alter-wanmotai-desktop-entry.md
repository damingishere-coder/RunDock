# Alter 万模台桌面软件入口任务

## 背景

Alter 当前把 `WanMoTai-Frontend` 作为受管开发进程启动，并根据监听端口显示“网页”入口。WanMoTai 将改为独立 Windows 软件，Alter 只保留一个外部桌面软件卡片，不再管理其进程。

Alter 工作树已有大量未提交的中文化、项目分组、网页入口和 Windows 进程修复。本任务必须在这些修改上增量实现，不 reset、stash、覆盖或整理无关代码。

## 目标

1. 项目支持 `managed` 与 `desktop` 两种 kind，旧数据保持 `managed`。
2. 桌面项目通过受校验的自定义协议 URI 显示“打开软件”。
3. 桌面项目即使没有进程成员也继续显示，但不提供端口、组件、资源指标或生命周期操作。
4. 将本机 WanMoTai 项目切换为 `desktop + wanmotai://open`，再删除旧开发进程。

## 允许修改范围

- 项目模型、项目持久化、项目 API/校验及对应 Rust 测试。
- 项目 TypeScript 类型、API 客户端、Projects 页面、入口组件及对应前端测试。
- 与新字段直接相关的 API 文档、本任务文件、构建产物和本机可恢复安装备份。
- 验证通过后备份并替换 `C:\Program Files\alter\alter.exe`，备份项目/进程状态后迁移本机 WanMoTai 项目。

## 禁止修改范围

- 不改变其他 managed 项目的网页入口、进程启停、日志、端口或内部名称。
- 不允许 `http`、`https`、`file`、`javascript`、`data` URI 作为桌面入口，也不直接执行任意路径/命令。
- 不读取或输出密码、Token、Cookie、`.env`、浏览器数据或其他 Secret。
- 不停止或删除 WanMoTai 以外的业务进程，不提交、push、重写历史或发布远程版本。
- Luna 子代理不得修改生产源码、安装二进制、编辑 `%APPDATA%` 或操作项目外路径。

## 已确定实现要求

- `ProjectRecord/ProjectInfo` 增加 `kind: managed | desktop` 与可选 `launch_uri`；旧 JSON 缺字段时默认 managed。
- `ProjectStatus` 增加 `desktop`；桌面项目 `enabled=true`、成员/CPU/内存/进程数均为零。
- 项目列表合并受管进程分组与项目存储中的 desktop-only 记录，不显示遗留的无进程 managed 记录。
- 桌面 URI 最大长度受限，只允许带 `://` 的自定义 scheme，拒绝网页、文件和脚本 scheme；UI 仅生成普通外部协议链接。
- 桌面项目的 start/stop/restart API 返回明确 409；UI 隐藏对应按钮和技术指标。
- managed 项目继续使用现有 WebPortButton，现有 API 和兼容行为不变。
- 先安装支持新字段的 Alter，再 PATCH WanMoTai 项目，最后删除 `WanMoTai-Frontend`；顺序不得颠倒。

## 验收标准

- WanMoTai 卡片保留在“常用”，显示“桌面软件”和“打开软件”，不再显示网页或启停入口。
- 点击只触发 `wanmotai://open`；桌面项目生命周期 API 被拒绝。
- 其他项目数量、分类、状态、网页按钮和技术展开内容不变。
- 守护进程重启后 desktop 项目仍存在，旧 projects.json 可无损加载。

## 测试命令

```powershell
cd web-ui
npm run format:check
npm run lint
npm test -- --run
npm run build
cd ..
cargo test --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
git diff --check
```

## 返回格式

- 修改文件和接口行为摘要。
- 前端/Rust 测试与构建证据，既有失败单列。
- Alter 二进制、项目元数据和进程状态备份位置。
- 本机入口迁移、真实点击验收、其他项目回归和剩余风险。
