# QQ Study Bridge 原生端口迁移任务

## 背景

`%USERPROFILE%\Documents\New project` 的 Streamlit Web 已由 Alter 原生运行在 8501。QQ Bridge 原生端口当前为 8765，与 Niuma Studio 的 Windows 发布 Worker 冲突。8764 当前无监听、无仓库约定冲突。

仓库已有用户修改：`app.py`、`mac_sender_client.py`、`video_scanner.py`，以及未跟踪 Docker 文件和 `front/`。必须局部合并，不得覆盖或回滚。

## 目标

- QQ Bridge 的 Windows 原生日常端口改为 8764。
- Docker 回退容器内部仍监听 8765，但主机映射改为 `8764:8765`，从而可与 Niuma 8765 并存。
- Streamlit 在原生模式检查 8764，在 Docker 容器内通过 `BRIDGE_API_PORT=8765` 检查内部 Bridge。

## 允许修改范围

- `app.py`（仅 Bridge 端口/URL/提示的局部修改）
- `config.yaml`（仅 callback 端口）
- `bridge_api.py`（仅默认示例端口）
- `start_bridge_api.bat`
- `start_all_services.ps1`
- `stop_all_services.ps1`
- `launcher_control_panel.ps1`
- `docker-compose.yml`（仅主机端口映射与 Streamlit 进程环境变量）
- 当前使用说明：`AGENTS.md`、`README.md`、`README_LOCAL_RUN.md`、`ONE_CLICK_START.md`、`NEXT_STEPS.md`、`MACBOOK_QQ_SENDER_PROMPT.md`
- 新增一个不导入业务模块、不访问真实数据库的静态/纯配置测试

## 禁止修改范围

- `.env`、数据库、日志、真实资料包、Mac/浏览器登录数据
- `mac_sender_client.py`、`video_scanner.py`、`front/`
- `DEVELOPMENT_LOG.md` 和 `DOCKER_RUN.md` 的历史记录
- `docker-entrypoint.sh` 与 Dockerfile 内部端口 8765
- Git commit/push/stash/reset/历史重写、Docker 状态变更、其他项目或 `QQ` 子目录

## 已确定实现要求

1. `app.py` 使用单一常量/纯函数读取 `BRIDGE_API_PORT`，默认 8764；非法值回退到 8764。健康 URL、界面状态、启动命令和本机示例统一由该值生成，避免散落硬编码。
2. `config.yaml` 的 `windows_callback_url` 改为主机可访问的 8764。
3. 原生启动/停止/控制面板端口、日志名、提示统一到 8764；保留现有项目身份校验，不扩大盲目停止范围。
4. Compose 主机映射改为 `8764:8765`，容器环境设置 `BRIDGE_API_PORT=8765`；容器内部 entrypoint 与 EXPOSE 保持 8765。
5. 当前使用说明改为原生 8764；历史日志和旧 Docker 专门文档不做机械替换。
6. 测试不得导入 `bridge_api.py`，避免初始化真实数据库；至少静态确认原生文件使用 8764、Compose 映射/环境正确、容器内部脚本仍 8765、app 无散落的固定 health URL。

## 验收标准

- 原生 Bridge 在 8764 启动，`/api/health` 返回 200。
- Niuma Worker 继续占用 8765，二者可同时运行。
- Streamlit 进程默认检查 8764。
- Docker 回退主机端口 8764、容器内部 8765，配置语义一致。
- 无范围外修改，无 secrets 或真实数据库写入。

## 测试命令

- `git diff --check`
- PowerShell 5.1/7 解析相关 `.ps1`
- `launcher_control_panel.ps1 -Mode SelfTest`
- 新增静态/纯配置测试
- 主代理随后做真实 8764 健康冒烟并交给 Alter 托管

## 返回格式

- `status`
- `changedFiles`
- `implementationSummary`
- `testSummary`
- `risks`
- `reviewTargets`
- `blockers`
