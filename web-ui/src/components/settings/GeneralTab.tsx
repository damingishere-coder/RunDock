// @group BusinessLogic > GeneralTab : General settings — polling, behaviour, storage, daemon, updates

import { useEffect, useState } from 'react'
import {
  ArrowDownToLine,
  Check,
  ChevronDown,
  ChevronUp,
  Loader,
  RefreshCw,
  RotateCcw,
} from 'lucide-react'
import type { UpdateInfo } from '@/types'
import type { AppSettings } from '@/lib/settings'
import { LOG_TAIL_OPTIONS, REFRESH_INTERVAL_OPTIONS } from '@/lib/settings'
import { api } from '@/lib/api'
import { NamespaceInput } from '@/components/NamespaceInput'
import { CopyPath, SettingRow, Toggle } from './shared'
import {
  card,
  descStyle,
  inputStyle,
  labelStyle,
  rowStyle,
  sectionTitle,
  selectStyle,
} from './sharedStyles'

interface Props {
  settings: AppSettings
  onUpdate: (patch: Partial<AppSettings>) => void
}

export default function GeneralTab({ settings, onUpdate }: Props) {
  const [sysPaths, setSysPaths] = useState<{ data_dir: string; log_dir: string } | null>(null)
  const [sysPathsError, setSysPathsError] = useState<string | null>(null)
  const [restarting, setRestarting] = useState(false)
  const [restartStatus, setRestartStatus] = useState<'idle' | 'restarting' | 'done' | 'error'>(
    'idle'
  )
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null)
  const [updateChecking, setUpdateChecking] = useState(false)
  const [updateStatus, setUpdateStatus] = useState<'idle' | 'updating' | 'done' | 'error'>('idle')
  const [updateError, setUpdateError] = useState<string | null>(null)
  const [releaseNotesOpen, setReleaseNotesOpen] = useState(false)

  useEffect(() => {
    api
      .getSystemPaths()
      .then(paths => {
        setSysPaths(paths)
        setSysPathsError(null)
      })
      .catch(error => {
        setSysPathsError(error instanceof Error ? error.message : '读取系统路径失败')
      })
  }, [])

  // @group BusinessLogic > Daemon : Restart daemon and poll until it comes back
  async function handleRestartDaemon() {
    setRestarting(true)
    setRestartStatus('restarting')
    try {
      await api.restartDaemon()
      let ok = false
      for (let i = 0; i < 25; i++) {
        await new Promise(r => setTimeout(r, 600))
        try {
          await api.getHealth()
          ok = true
          break
        } catch {
          /* not up yet */
        }
      }
      setRestartStatus(ok ? 'done' : 'error')
    } catch {
      setRestartStatus('error')
    } finally {
      setRestarting(false)
      setTimeout(() => setRestartStatus('idle'), 3000)
    }
  }

  // @group BusinessLogic > Update : Check for a newer version on GitHub
  async function handleCheckUpdate() {
    setUpdateChecking(true)
    setUpdateError(null)
    try {
      const info = await api.checkUpdate()
      setUpdateInfo(info)
      if (info.error) setUpdateError(info.error)
    } catch (e: unknown) {
      setUpdateError(e instanceof Error ? e.message : '检查失败')
    } finally {
      setUpdateChecking(false)
    }
  }

  // @group BusinessLogic > Update : Download and apply the update, then reconnect
  async function handleApplyUpdate() {
    if (!updateInfo?.download_url || !updateInfo.integrity_verified) return
    setUpdateStatus('updating')
    setUpdateError(null)
    try {
      await api.applyUpdate()
      setUpdateStatus('done')
    } catch (error: unknown) {
      setUpdateStatus('error')
      setUpdateError(error instanceof Error ? error.message : '更新失败')
    }
  }

  return (
    <>
      <p style={sectionTitle}>轮询与刷新</p>
      <div style={card}>
        <SettingRow
          label="自动刷新"
          description="自动轮询守护进程以获取进程更新。"
          control={
            <Toggle checked={settings.autoRefresh} onChange={v => onUpdate({ autoRefresh: v })} />
          }
        />
        <SettingRow
          label="进程刷新间隔"
          description="进程列表的刷新频率。"
          control={
            <select
              aria-label="进程刷新间隔"
              value={settings.processRefreshInterval}
              onChange={e => onUpdate({ processRefreshInterval: Number(e.target.value) })}
              disabled={!settings.autoRefresh}
              style={{ ...selectStyle, opacity: settings.autoRefresh ? 1 : 0.4 }}
            >
              {REFRESH_INTERVAL_OPTIONS.map(o => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          }
        />
        <SettingRow
          label="健康检查间隔"
          description="侧边栏守护进程状态的轮询频率。"
          isLast
          control={
            <select
              aria-label="健康检查间隔"
              value={settings.healthRefreshInterval}
              onChange={e => onUpdate({ healthRefreshInterval: Number(e.target.value) })}
              style={selectStyle}
            >
              {REFRESH_INTERVAL_OPTIONS.map(o => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          }
        />
      </div>

      <p style={sectionTitle}>行为</p>
      <div style={card}>
        <SettingRow
          label="删除前确认"
          description="删除进程时显示确认对话框。"
          control={
            <Toggle
              checked={settings.confirmBeforeDelete}
              onChange={v => onUpdate({ confirmBeforeDelete: v })}
            />
          }
        />
        <SettingRow
          label="关闭前确认"
          description="关闭守护进程时显示确认对话框。"
          isLast
          control={
            <Toggle
              checked={settings.confirmBeforeShutdown}
              onChange={v => onUpdate({ confirmBeforeShutdown: v })}
            />
          }
        />
      </div>

      <p style={sectionTitle}>日志查看器</p>
      <div style={card}>
        <SettingRow
          label="默认日志行数"
          description="打开进程日志视图时获取的日志行数。"
          isLast
          control={
            <select
              aria-label="默认日志行数"
              value={settings.logTailLines}
              onChange={e => onUpdate({ logTailLines: Number(e.target.value) })}
              style={selectStyle}
            >
              {LOG_TAIL_OPTIONS.map(o => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          }
        />
      </div>

      <p style={sectionTitle}>进程默认值</p>
      <div style={card}>
        <SettingRow
          label="默认命名空间"
          description="创建新进程或定时任务时预填的命名空间。"
          isLast
          control={
            <NamespaceInput
              style={{ ...inputStyle, width: 140, fontSize: 12, padding: '5px 10px' }}
              value={settings.defaultNamespace}
              onChange={v => onUpdate({ defaultNamespace: v })}
              placeholder="default"
            />
          }
        />
      </div>

      <p style={sectionTitle}>存储</p>
      <div style={card}>
        <SettingRow
          label="数据目录"
          description="RunDock 存储状态、PID 和守护进程日志的根目录。"
          control={
            sysPaths ? (
              <CopyPath value={sysPaths.data_dir} />
            ) : sysPathsError ? (
              <span style={{ fontSize: 11, color: 'var(--color-destructive)' }}>
                {sysPathsError}
              </span>
            ) : (
              <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>加载中…</span>
            )
          }
        />
        <SettingRow
          label="日志目录"
          description={
            <>
              进程标准输出/错误输出日志的写入目录。可通过{' '}
              <code style={{ fontSize: 10, fontFamily: 'monospace' }}>ALTER_LOG_DIR</code>{' '}
              环境变量覆盖。
            </>
          }
          isLast
          control={
            sysPaths ? (
              <CopyPath value={sysPaths.log_dir} />
            ) : sysPathsError ? (
              <span style={{ fontSize: 11, color: 'var(--color-destructive)' }}>
                {sysPathsError}
              </span>
            ) : (
              <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>加载中…</span>
            )
          }
        />
      </div>

      <p style={sectionTitle}>守护进程</p>
      <div style={card}>
        <SettingRow
          label="重启守护进程"
          description="重启 RunDock 守护进程。正在运行的进程会继续运行，只有 HTTP 服务会短暂重启。"
          isLast
          control={
            <button
              onClick={handleRestartDaemon}
              disabled={restarting}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 7,
                padding: '6px 14px',
                fontSize: 12,
                fontWeight: 500,
                background:
                  restartStatus === 'done'
                    ? 'var(--color-status-running)'
                    : restartStatus === 'error'
                      ? 'var(--color-destructive)'
                      : 'var(--color-secondary)',
                color: restartStatus === 'idle' ? 'var(--color-foreground)' : '#fff',
                border: '1px solid var(--color-border)',
                borderRadius: 6,
                cursor: restarting ? 'default' : 'pointer',
                opacity: restarting ? 0.7 : 1,
                transition: 'background 0.2s',
              }}
            >
              {restarting ? (
                <>
                  <Loader size={12} style={{ animation: 'spin 1s linear infinite' }} /> 重启中…
                </>
              ) : restartStatus === 'done' ? (
                <>
                  <Check size={12} /> 已恢复连接
                </>
              ) : restartStatus === 'error' ? (
                '连接失败'
              ) : (
                <>
                  <RotateCcw size={12} /> 重启守护进程
                </>
              )}
            </button>
          }
        />
      </div>

      <p style={sectionTitle}>更新</p>
      <div style={card}>
        <div
          style={{
            ...rowStyle,
            borderBottom:
              updateInfo && !updateInfo.up_to_date ? '1px solid var(--color-border)' : 'none',
            paddingBottom: updateInfo && !updateInfo.up_to_date ? 10 : 0,
          }}
        >
          <div style={{ flex: 1, paddingRight: 24 }}>
            <div style={labelStyle}>应用版本</div>
            <div style={descStyle}>
              当前版本：
              <code style={{ fontFamily: 'monospace', fontSize: 11 }}>
                {updateInfo?.current ?? '…'}
              </code>
              {updateInfo && !updateInfo.up_to_date && (
                <span style={{ marginLeft: 8, color: '#f97316', fontWeight: 600 }}>
                  → v{updateInfo.latest} 可用
                </span>
              )}
              {updateInfo?.up_to_date && (
                <span style={{ marginLeft: 8, color: 'var(--color-status-running)' }}>
                  ✓ 已是最新版本
                </span>
              )}
            </div>
            {updateError && (
              <div style={{ ...descStyle, color: 'var(--color-destructive)', marginTop: 4 }}>
                {updateError}
              </div>
            )}
          </div>
          <button
            onClick={handleCheckUpdate}
            disabled={updateChecking || updateStatus === 'updating'}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '6px 14px',
              fontSize: 12,
              fontWeight: 500,
              background: 'var(--color-secondary)',
              color: 'var(--color-foreground)',
              border: '1px solid var(--color-border)',
              borderRadius: 6,
              cursor: updateChecking ? 'default' : 'pointer',
              opacity: updateChecking ? 0.6 : 1,
              flexShrink: 0,
            }}
          >
            {updateChecking ? (
              <>
                <Loader size={12} style={{ animation: 'spin 1s linear infinite' }} /> 检查中…
              </>
            ) : (
              <>
                <RefreshCw size={12} /> 检查更新
              </>
            )}
          </button>
        </div>

        {updateInfo && !updateInfo.up_to_date && (
          <div style={{ paddingTop: 12 }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 12,
                marginBottom: 10,
              }}
            >
              <div>
                <div style={{ fontSize: 13, fontWeight: 600, color: '#f97316' }}>
                  v{updateInfo.latest} 可用
                </div>
                {updateInfo.published_at && (
                  <div style={descStyle}>
                    发布于 {new Date(updateInfo.published_at).toLocaleDateString('zh-CN')}
                  </div>
                )}
                {(!updateInfo.download_url || !updateInfo.integrity_verified) && (
                  <div style={{ ...descStyle, color: 'var(--color-destructive)', marginTop: 2 }}>
                    自动更新缺少受信校验信息 — 请从 GitHub 手动更新。
                  </div>
                )}
              </div>
              {updateInfo.download_url && updateInfo.integrity_verified && (
                <button
                  onClick={handleApplyUpdate}
                  disabled={updateStatus === 'updating' || updateStatus === 'done'}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 7,
                    padding: '7px 16px',
                    fontSize: 12,
                    fontWeight: 600,
                    background:
                      updateStatus === 'done'
                        ? 'var(--color-status-running)'
                        : updateStatus === 'error'
                          ? 'var(--color-destructive)'
                          : 'var(--color-primary)',
                    color: '#fff',
                    border: 'none',
                    borderRadius: 6,
                    cursor:
                      updateStatus === 'updating' || updateStatus === 'done'
                        ? 'default'
                        : 'pointer',
                    opacity: updateStatus === 'updating' ? 0.75 : 1,
                    flexShrink: 0,
                  }}
                >
                  {updateStatus === 'updating' ? (
                    <>
                      <Loader size={12} style={{ animation: 'spin 1s linear infinite' }} /> 下载中…
                    </>
                  ) : updateStatus === 'done' ? (
                    <>
                      <Check size={12} />{' '}
                      {updateInfo.is_installer ? '安装程序已启动' : '重新加载中…'}
                    </>
                  ) : updateStatus === 'error' ? (
                    '失败 — 重试？'
                  ) : updateInfo.is_installer ? (
                    <>
                      <ArrowDownToLine size={12} /> 下载并安装
                    </>
                  ) : (
                    <>
                      <ArrowDownToLine size={12} /> 立即更新
                    </>
                  )}
                </button>
              )}
            </div>

            {updateInfo.release_notes && (
              <div>
                <button
                  onClick={() => setReleaseNotesOpen(o => !o)}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 4,
                    background: 'transparent',
                    border: 'none',
                    cursor: 'pointer',
                    fontSize: 11,
                    color: 'var(--color-muted-foreground)',
                    padding: 0,
                    marginBottom: 6,
                  }}
                >
                  {releaseNotesOpen ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                  发布说明
                </button>
                {releaseNotesOpen && (
                  <pre
                    style={{
                      fontSize: 11,
                      fontFamily: 'monospace',
                      background: 'var(--color-muted)',
                      border: '1px solid var(--color-border)',
                      borderRadius: 4,
                      padding: '8px 10px',
                      margin: 0,
                      whiteSpace: 'pre-wrap',
                      wordBreak: 'break-word',
                      maxHeight: 200,
                      overflow: 'auto',
                      color: 'var(--color-foreground)',
                    }}
                  >
                    {updateInfo.release_notes}
                  </pre>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      {import.meta.env.DEV && (
        <>
          <p style={sectionTitle}>开发者</p>
          <div style={card}>
            <SettingRow
              label="React Query 开发工具"
              description="显示查询检查器面板以调试 API 缓存状态。"
              isLast
              control={
                <Toggle
                  checked={settings.showQueryDevtools}
                  onChange={v => onUpdate({ showQueryDevtools: v })}
                />
              }
            />
          </div>
        </>
      )}

      <p
        style={{
          fontSize: 11,
          color: 'var(--color-muted-foreground)',
          textAlign: 'center',
          marginTop: 8,
        }}
      >
        设置存储在守护进程数据目录中，并会在会话之间保留。 更改会立即生效。
      </p>
    </>
  )
}
