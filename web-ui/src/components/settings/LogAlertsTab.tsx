// @group BusinessLogic > LogAlertsTab : Log alert settings — global thresholds and namespace overrides

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { NamespaceInput } from '@/components/NamespaceInput'
import { SettingRow, Toggle } from './shared'
import { card, inputStyle, sectionTitle } from './sharedStyles'

export default function LogAlertsTab() {
  const [laEnabled, setLaEnabled] = useState(false)
  const [laThreshold, setLaThreshold] = useState(10)
  const [laCooldown, setLaCooldown] = useState(15)
  const [laCheckInterval, setLaCheckInterval] = useState(5)
  const [laNsOverrides, setLaNsOverrides] = useState<
    Record<
      string,
      {
        enabled?: boolean
        stderr_threshold?: number
        cooldown_mins?: number
        check_interval_mins?: number
      }
    >
  >({})
  const [laNsNew, setLaNsNew] = useState('')
  const [laSaving, setLaSaving] = useState(false)
  const [laSaved, setLaSaved] = useState(false)
  const [laError, setLaError] = useState<string | null>(null)
  const [settingsLoaded, setSettingsLoaded] = useState(false)
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null)

  useEffect(() => {
    api
      .getLogAlerts()
      .then(store => {
        setLaEnabled(store.global.enabled)
        setLaThreshold(store.global.stderr_threshold)
        setLaCooldown(store.global.cooldown_mins)
        setLaCheckInterval(store.global.check_interval_mins ?? 5)
        setLaNsOverrides(store.namespaces ?? {})
        setSettingsLoaded(true)
      })
      .catch(error => {
        setSettingsLoadError(error instanceof Error ? error.message : '读取日志告警设置失败')
      })
  }, [])

  if (settingsLoadError) {
    return (
      <div role="alert" style={{ color: 'var(--color-destructive)', padding: 16 }}>
        日志告警设置加载失败，已禁止保存默认值：{settingsLoadError}
      </div>
    )
  }
  if (!settingsLoaded) {
    return <div style={{ padding: 16, color: 'var(--color-muted-foreground)' }}>加载中…</div>
  }

  return (
    <>
      <p style={sectionTitle}>全局设置</p>
      <div style={card}>
        <SettingRow
          label="启用日志告警"
          description="检查间隔内的 stderr 行数超过阈值时发送通知"
          control={<Toggle checked={laEnabled} onChange={setLaEnabled} />}
        />
        <SettingRow
          label="检查间隔"
          description="守护进程扫描日志突增的频率"
          control={
            <select
              aria-label="日志告警检查间隔"
              value={laCheckInterval}
              onChange={e => setLaCheckInterval(Number(e.target.value))}
              style={{ ...inputStyle, width: 140 }}
            >
              <option value={1}>1 分钟</option>
              <option value={2}>2 分钟</option>
              <option value={5}>5 分钟</option>
              <option value={10}>10 分钟</option>
              <option value={15}>15 分钟</option>
              <option value={30}>30 分钟</option>
              <option value={60}>1 小时</option>
            </select>
          }
        />
        <SettingRow
          label="Stderr 阈值"
          description="单个检查间隔内出现此数量的 stderr 行时告警"
          control={
            <input
              aria-label="日志告警 stderr 阈值"
              type="number"
              min={1}
              max={10000}
              value={laThreshold}
              onChange={e => setLaThreshold(Math.max(1, Number(e.target.value)))}
              style={{ ...inputStyle, width: 80, textAlign: 'right' }}
            />
          }
        />
        <SettingRow
          label="冷却时间"
          description="同一进程重复告警之间的最短时间"
          isLast
          control={
            <select
              aria-label="日志告警冷却时间"
              value={laCooldown}
              onChange={e => setLaCooldown(Number(e.target.value))}
              style={{ ...inputStyle, width: 140 }}
            >
              <option value={5}>5 分钟</option>
              <option value={10}>10 分钟</option>
              <option value={15}>15 分钟</option>
              <option value={30}>30 分钟</option>
              <option value={60}>1 小时</option>
            </select>
          }
        />
      </div>

      <p style={sectionTitle}>命名空间覆盖</p>
      <p
        style={{
          fontSize: 12,
          color: 'var(--color-muted-foreground)',
          marginTop: -8,
          marginBottom: 12,
        }}
      >
        为指定命名空间覆盖全局设置。字段留空则继承全局值。
      </p>

      {Object.keys(laNsOverrides).length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 12 }}>
          {Object.entries(laNsOverrides).map(([ns, ov]) => (
            <div
              key={ns}
              style={{
                ...card,
                padding: '12px 16px',
                marginBottom: 0,
                display: 'flex',
                flexWrap: 'wrap',
                gap: 12,
                alignItems: 'center',
              }}
            >
              <span
                style={{
                  fontSize: 13,
                  fontWeight: 600,
                  minWidth: 120,
                  color: 'var(--color-foreground)',
                }}
              >
                📁 {ns}
              </span>
              <label
                style={{
                  fontSize: 12,
                  color: 'var(--color-muted-foreground)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                }}
              >
                启用
                <select
                  value={ov.enabled === true ? 'yes' : ov.enabled === false ? 'no' : 'inherit'}
                  onChange={e => {
                    const v = e.target.value
                    setLaNsOverrides(prev => ({
                      ...prev,
                      [ns]: { ...prev[ns], enabled: v === 'inherit' ? undefined : v === 'yes' },
                    }))
                  }}
                  style={{ ...inputStyle, width: 100, padding: '3px 6px' }}
                >
                  <option value="inherit">继承</option>
                  <option value="yes">是</option>
                  <option value="no">否</option>
                </select>
              </label>
              <label
                style={{
                  fontSize: 12,
                  color: 'var(--color-muted-foreground)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                }}
              >
                阈值
                <input
                  type="number"
                  min={1}
                  max={10000}
                  placeholder="继承"
                  value={ov.stderr_threshold ?? ''}
                  onChange={e => {
                    const v =
                      e.target.value === '' ? undefined : Math.max(1, Number(e.target.value))
                    setLaNsOverrides(prev => ({
                      ...prev,
                      [ns]: { ...prev[ns], stderr_threshold: v },
                    }))
                  }}
                  style={{ ...inputStyle, width: 80, textAlign: 'right', padding: '3px 6px' }}
                />
              </label>
              <label
                style={{
                  fontSize: 12,
                  color: 'var(--color-muted-foreground)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                }}
              >
                冷却时间
                <select
                  value={ov.cooldown_mins ?? ''}
                  onChange={e => {
                    const v = e.target.value === '' ? undefined : Number(e.target.value)
                    setLaNsOverrides(prev => ({ ...prev, [ns]: { ...prev[ns], cooldown_mins: v } }))
                  }}
                  style={{ ...inputStyle, width: 120, padding: '3px 6px' }}
                >
                  <option value="">继承</option>
                  <option value={5}>5 分钟</option>
                  <option value={10}>10 分钟</option>
                  <option value={15}>15 分钟</option>
                  <option value={30}>30 分钟</option>
                  <option value={60}>1 小时</option>
                </select>
              </label>
              <label
                style={{
                  fontSize: 12,
                  color: 'var(--color-muted-foreground)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                }}
              >
                检查间隔
                <select
                  value={ov.check_interval_mins ?? ''}
                  onChange={e => {
                    const v = e.target.value === '' ? undefined : Number(e.target.value)
                    setLaNsOverrides(prev => ({
                      ...prev,
                      [ns]: { ...prev[ns], check_interval_mins: v },
                    }))
                  }}
                  style={{ ...inputStyle, width: 120, padding: '3px 6px' }}
                >
                  <option value="">继承</option>
                  <option value={1}>1 分钟</option>
                  <option value={2}>2 分钟</option>
                  <option value={5}>5 分钟</option>
                  <option value={10}>10 分钟</option>
                  <option value={15}>15 分钟</option>
                </select>
              </label>
              <button
                onClick={() =>
                  setLaNsOverrides(prev => {
                    const next = { ...prev }
                    delete next[ns]
                    return next
                  })
                }
                style={{
                  marginLeft: 'auto',
                  fontSize: 12,
                  padding: '3px 10px',
                  background: 'transparent',
                  border: '1px solid var(--color-border)',
                  borderRadius: 5,
                  cursor: 'pointer',
                  color: 'var(--color-status-crashed)',
                }}
              >
                移除
              </button>
            </div>
          ))}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 16 }}>
        <NamespaceInput
          placeholder="命名空间名称"
          value={laNsNew}
          onChange={setLaNsNew}
          style={{ ...inputStyle, width: 180 }}
        />
        <button
          disabled={!laNsNew.trim() || laNsNew.trim() in laNsOverrides}
          onClick={() => {
            const ns = laNsNew.trim()
            if (!ns || ns in laNsOverrides) return
            setLaNsOverrides(prev => ({ ...prev, [ns]: {} }))
            setLaNsNew('')
          }}
          style={{
            padding: '6px 14px',
            fontSize: 13,
            background: 'var(--color-secondary)',
            border: '1px solid var(--color-border)',
            borderRadius: 6,
            cursor: 'pointer',
            color: 'var(--color-foreground)',
          }}
        >
          + 添加命名空间
        </button>
      </div>

      <div
        style={{
          ...card,
          background: 'rgba(var(--color-primary-rgb,99,102,241),0.05)',
          borderColor: 'rgba(var(--color-primary-rgb,99,102,241),0.2)',
          marginBottom: 16,
        }}
      >
        <p
          style={{
            fontSize: 12,
            color: 'var(--color-muted-foreground)',
            margin: 0,
            lineHeight: 1.7,
          }}
        >
          告警会通过你配置的 <strong>Webhook / Slack / Teams</strong> 和 <strong>Telegram</strong>{' '}
          渠道发送。 可通过 API 为进程设置级别覆盖（进程上的 <code>log_alert</code> 字段）。
        </p>
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button
          onClick={async () => {
            setLaSaving(true)
            setLaSaved(false)
            setLaError(null)
            try {
              await api.updateLogAlerts({
                global: {
                  enabled: laEnabled,
                  stderr_threshold: laThreshold,
                  cooldown_mins: laCooldown,
                  check_interval_mins: laCheckInterval,
                },
                namespaces: laNsOverrides,
              })
              setLaSaved(true)
              setTimeout(() => setLaSaved(false), 2500)
            } catch (e: unknown) {
              setLaError(e instanceof Error ? e.message : '保存失败')
            } finally {
              setLaSaving(false)
            }
          }}
          disabled={laSaving}
          style={{
            padding: '7px 18px',
            fontSize: 13,
            fontWeight: 500,
            background: laSaved ? 'var(--color-status-running)' : 'var(--color-primary)',
            color: '#fff',
            border: 'none',
            borderRadius: 6,
            cursor: 'pointer',
            opacity: laSaving ? 0.6 : 1,
            transition: 'background 0.2s',
          }}
        >
          {laSaved ? '已保存！' : laSaving ? '保存中…' : '保存'}
        </button>
        {laError && (
          <span style={{ fontSize: 12, color: 'var(--color-status-crashed)' }}>{laError}</span>
        )}
      </div>
    </>
  )
}
