# Zhihu Summary Workbench 本地内存队列执行任务

## 背景

`%USERPROFILE%\Documents\zhihu-summary-workbench` 当前的本地开发流程会启动 Docker Redis，并让 FastAPI 与 Worker 分进程运行。项目虽然已有 `QUEUE_BACKEND=memory`，但 API 与独立 Worker 会分别创建内存 Broker，导致 API 入队的任务无法被 Worker 消费。

仓库已有用户修改，必须保留且不得覆盖：`.env.example`、`CODEX_IMPLEMENTATION_SPEC.md`、`backend/app/core/config.py`、`backend/tests/test_settings.py`。

## 目标

让 `QUEUE_BACKEND=memory` 成为无需 Docker 的日常本地开发模式：FastAPI lifespan 启动一个共享同一 Broker 的进程内 Worker，并在关闭时优雅停止。Docker Compose、Redis 和独立 Worker 继续作为完整集成/部署回退。

## 允许修改范围

- `backend/app/main.py`
- `backend/app/worker/main.py`
- 新增与共享 Worker 生命周期直接相关的后端测试文件
- 新增 `scripts/start-local.ps1`、`scripts/stop-local.ps1`
- `README.md` 中仅与本地/Docker启动方式相关的小范围说明

## 禁止修改范围

- `.env`、`.env.example`、`backend/app/core/config.py`、`backend/tests/test_settings.py`
- `docker-compose.yml`、Dockerfile、前端源码和视觉设计
- 业务抓取、总结、任务处理逻辑
- Git 提交、push、stash、reset、历史重写
- secrets、浏览器数据、生产数据或其他工作区

## 已确定实现要求

1. 把 Worker 循环抽成可复用的 async 函数，允许外部传入 Broker、停止事件和是否拥有资源的标志；独立 Worker 的现有 CLI 行为必须保持。
2. `QUEUE_BACKEND=memory` 时，FastAPI lifespan 必须把 `app.state.broker` 的同一实例传给进程内 Worker。
3. 关闭时先发出停止信号，再等待 Worker 在有界时间内退出；必要时取消任务，不得无限悬挂。
4. 进程内 Worker 不得关闭由 FastAPI 拥有的 Broker，也不得重复 dispose 数据库引擎；独立 Worker 仍负责关闭自己创建的资源。
5. Redis 模式不得在 API 内启动第二个 Worker，Docker Compose 原有独立 Worker 架构保持不变。
6. `scripts/start-local.ps1` 以显式进程环境变量设置 `QUEUE_BACKEND=memory`，启动迁移、FastAPI `127.0.0.1:8000` 与 Vite `127.0.0.1:4173`，不调用 Docker，也不启动独立 Worker。使用项目内 `.venv` 和已有 `node_modules`，缺失时给清晰错误。
7. `scripts/stop-local.ps1` 只能依据本项目记录的 PID、项目根、命令行/端口等信息停止本批原生进程，不得按端口盲杀，不得调用 Docker。
8. PID/状态与日志放入已忽略的本地运行目录；若现有 ignore 不覆盖，可最小修改 `.gitignore`，但需先确认不会覆盖用户改动。
9. 测试至少覆盖：memory 模式启动共享 Worker、Redis 模式不内嵌 Worker、关闭后后台任务结束、Broker/数据库资源所有权不重复释放。

## 验收标准

- memory 模式 API 入队后，同一 Broker 可被内嵌 Worker 消费。
- 应用 lifespan 退出不会残留 Worker task。
- Redis 模式行为与当前 Docker 架构兼容。
- 本地脚本完全不调用 Docker。
- 不修改上述用户脏文件，不产生范围外改动或 secrets。

## 测试命令

- `git diff --check`
- `.\.venv\Scripts\python.exe -m pytest backend\tests\<新增测试文件> backend\tests\test_queue.py`
- PowerShell 5.1 语法解析新增脚本
- 如风险可控，再运行 `.\.venv\Scripts\python.exe -m pytest`
- `frontend\npm.cmd` 或系统 `npm.cmd run typecheck/test:run/build` 仅当实现触及启动联调且依赖已存在

## 返回格式

- `status`
- `changedFiles`
- `implementationSummary`
- `testSummary`
- `risks`
- `reviewTargets`
- `blockers`
