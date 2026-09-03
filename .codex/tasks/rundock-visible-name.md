# RunDock 对外名称统一

## 背景

项目已经以 RunDock 作为仓库、安装包、桌面端和网页品牌，但 CLI 输出、后台日志、
少量界面提示、安装说明和公开文档仍把兼容实现名 `alter` 当作产品名称展示，容易让
用户误以为这是两个项目。

## 目标

- 所有面向用户的产品名称、输出前缀、界面文案和说明统一为 RunDock。
- `alter` 只作为必须保留的兼容命令或技术标识出现，不再作为产品品牌出现。
- 不破坏现有安装升级、托管项目、脚本、数据目录或 Linux 服务。

## 允许修改范围

- Rust CLI、daemon、API 与桌面壳中的用户可见文案及相关测试。
- Web UI 的用户可见名称、错误提示及相关测试。
- Windows 安装器、Linux 包说明、systemd、发布工作流与质量工具的显示描述。
- 当前工程验收报告中的项目名称。
- 根 README 与 `docs/` 下的公开说明文字。
- 本任务说明文件。

## 禁止修改范围

- 不重命名 `alter.exe`、`alter` CLI、Rust crate/lib/bin 或 Debian 包。
- 不修改 `ALTER_*` 环境变量、`alter-pm2` 数据目录、`alter-daemon.service`、
  Prometheus 指标、浏览器存储键、协议头、Inno AppId 或上游 Winget 历史清单。
- 不修改用户的 `state.json`、`projects.json`、数据库或任何受托管项目状态。
- 不重启 daemon，不部署，不迁移当前工作目录。

## 已确定实现要求

- CLI 的可见前缀统一为 `[RunDock]`，产品错误和日志使用 `RunDock daemon`。
- 实际可执行命令示例继续使用兼容命令 `alter ...`，但说明文字只称 RunDock CLI。
- Web 状态栏只显示 `RunDock` 版本，不显示 `alter CLI`。
- 浏览器存储损坏提示不得要求用户操作内部键名。
- 安装器和包元数据仅展示 RunDock；技术路径、服务名和命令保持原值。
- 公开文档把产品主语统一为 RunDock，代码块中的兼容命令和路径保持原值。

## 验收标准

- 目标文件中不再出现 `[alter]`、`compatible alter CLI`、`alter CLI）`、
  `Alter data`、`another Alter daemon` 等旧品牌展示。
- 所有兼容命令、路径、服务名、存储键和环境变量保持不变。
- 前端构建、测试和 lint 通过；新增文件通过 Prettier，已有的全仓格式基线问题准确记录。
- Rust 格式、检查、测试和 Clippy 通过；如发布构建受本机环境阻塞，返回准确证据。
- 最终 diff 只包含本任务文件和明确的用户可见文案修改。

## 测试命令

```powershell
Push-Location web-ui
npm run build
npm test
npm run lint
npm run format:check
Pop-Location

cargo fmt -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## 返回格式

报告统一后的命名边界、修改文件、兼容项保留情况、验证结果、提交、推送与 PR 状态。
