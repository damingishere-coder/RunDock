# Docker 本地解绑执行任务

## 背景

用户希望多个历史项目的日常开发不再依赖 Docker，默认由 Windows 本机进程和 Alter 管理；Dockerfile 与 Compose 配置继续保留用于完整集成、部署和回退。当前唯一运行中的 Compose 项目是 `%USERPROFILE%\Documents\New project 2`（`niuma-studio`）。多个目标仓库存在用户未提交修改，必须原样保留。

## 目标

1. 修复 Alter 保存/恢复进程时丢失完整 `AppConfig` 字段的问题。
2. 固化低风险项目的本机启动方式，并补齐缺失进程。
3. 将 Niuma 从运行中的 Compose 安全切换到本机 Uvicorn，不丢 SQLite 与任务文件。
4. 为知乎工作台提供 SQLite + 进程内内存队列的核心开发模式。
5. 保留所有 Docker 部署与回退能力；将废弃的 ModalDesk 目录移到回收站。

## 允许修改范围

- `%USERPROFILE%\Documents\Alter`：仅 Rust 状态持久化相关文件、对应测试、任务记录；不得改 Web UI 现有用户修改。
- `%USERPROFILE%\Documents\AI - GroupBrief`：本机启动配置/文档；优先复用现有启动器。
- `%USERPROFILE%\Documents\AI - gemini-webapi-proxy`：本机启动配置/文档；优先复用现有 Windows 管理脚本。
- `%USERPROFILE%\Documents\New project`：本机启动配置/文档，补齐 Bridge API。
- `%USERPROFILE%\Documents\New project 2`：本机启动配置/文档、主工作树的宿主路径配置。
- `%USERPROFILE%\Documents\webnovel-writer Skill\Novel-Codex-Writer`：增加本机 Node/Vite 默认入口，保留 Docker 回退入口。
- `%USERPROFILE%\Documents\New project 3`：本机启动说明，默认禁用。
- `%USERPROFILE%\Documents\AI-JobPilot-Cloud`：固化现有 Windows 原生模式与文档。
- `%USERPROFILE%\Documents\zhihu-summary-workbench`：内存队列本机 worker、启动配置、测试与文档。
- `%APPDATA%\alter-pm2\`：在验证后写入本机项目清单和持久状态，不得写入任何秘密值。

## 禁止修改范围

- 不得 reset、stash、checkout 覆盖、提交、push 或重写任何 Git 历史。
- 不得覆盖或删除用户已有未提交修改。
- 不得删除 Dockerfile、Compose 文件、镜像、数据卷或执行全局 prune。
- 不得读取或输出 `.env`、Cookie、Token、API Key、Codex `auth.json` 内容。
- 不得改动 Niuma 的其他三个 worktree。
- 子代理不得删除 ModalDesk、停止 Docker Desktop、操作项目外路径或执行 Docker 资源清理。

## 已确定实现要求

- Alter 保存状态时必须从受管进程保留完整 `AppConfig`，而不是只从 `ProcessInfo` 重建子集；运行时 PID/状态仍独立保存。
- 本机配置使用唯一进程名和 namespace，敏感变量只通过项目 `.env`/`env_file` 读取。
- GroupBrief 使用 8766/5173；Gemini Proxy 使用 4982；QQ Study 使用 8501/8765；Novel 原生模式使用 5174；AI-JobPilot-Cloud 使用 8888/6866；New project 3 默认禁用；知乎使用 8000/4173；Niuma 最终使用 8001。
- Niuma 先在临时端口和临时数据副本验证，再停止容器、备份 SQLite、切换 8001；不得并发写同一数据库。
- 知乎在 `QUEUE_BACKEND=memory` 时让 API lifespan 启动共享同一 broker 的进程内 worker；Redis 模式继续使用独立 worker。
- ModalDesk 仅由 Codex 主代理在最终复核后移入 Windows 回收站。

## 验收标准

- Alter 完整配置经过 save/restore 后，`env_file`、health check、hooks、watch_paths/watch_ignore 和 enabled 均不丢失。
- 各默认启用项目可在不调用 Docker 命令的情况下启动并通过健康/端口检查。
- Niuma 容器退出后，原生 8001 服务可用且原数据保留；Docker 配置仍可回退。
- 知乎内存模式任务可由同进程 worker 消费并在关闭时清理 worker task。
- 不产生范围外修改、秘密文件、临时调试文件或 TODO。

## 测试命令

- Alter：`cargo test`；必要时运行定向状态/配置测试。
- GroupBrief：`.venv\Scripts\python.exe -m pytest`、`npm run build`。
- Gemini Proxy：现有 Windows 测试入口和 pytest。
- QQ Study：现有测试（若有）及 8501/8765 健康检查。
- Niuma：`.venv\Scripts\python.exe -m pytest`，临时端口冒烟测试与切换后 8001 健康检查。
- Novel：`npm test`、`npm run build`。
- JobPilot：`gradlew.bat test`、前端现有 test/build。
- 知乎：后端 pytest、前端 `npm run test:run` 与 build、内存 worker 端到端测试。

## Worker 返回格式

每批必须返回：`status`、实际修改文件、实现摘要、执行的测试及结果、未执行测试及原因、发现的风险/阻塞、是否存在范围外修改。不得仅返回“完成”。
