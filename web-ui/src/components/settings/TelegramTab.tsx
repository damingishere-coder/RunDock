// @group BusinessLogic > TelegramTab : Telegram bot settings — token, chat IDs, notifications

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { SettingRow, Toggle } from './shared'
import { card, inputStyle, sectionTitle } from './sharedStyles'

export default function TelegramTab() {
  const [tgEnabled, setTgEnabled] = useState(false)
  const [tgToken, setTgToken] = useState('')
  const [tgTokenHint, setTgTokenHint] = useState<string | null>(null)
  const [tgTokenSet, setTgTokenSet] = useState(false)
  const [tgChatIds, setTgChatIds] = useState<string>('')
  const [tgNotifyCrash, setTgNotifyCrash] = useState(true)
  const [tgNotifyStart, setTgNotifyStart] = useState(false)
  const [tgNotifyStop, setTgNotifyStop] = useState(false)
  const [tgNotifyRestart, setTgNotifyRestart] = useState(true)
  const [tgSaving, setTgSaving] = useState(false)
  const [tgSaved, setTgSaved] = useState(false)
  const [tgError, setTgError] = useState<string | null>(null)
  const [tgBotInfo, setTgBotInfo] = useState<{
    ok: boolean
    username: string | null
    first_name: string | null
    error: string | null
  } | null>(null)
  const [tgValidating, setTgValidating] = useState(false)
  const [tgTesting, setTgTesting] = useState(false)
  const [tgTestResult, setTgTestResult] = useState<string | null>(null)
  const [tgChangingToken, setTgChangingToken] = useState(false)
  const [settingsLoaded, setSettingsLoaded] = useState(false)
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null)

  useEffect(() => {
    api
      .getTelegramConfig()
      .then(cfg => {
        setTgEnabled(cfg.enabled)
        setTgTokenHint(cfg.bot_token_hint)
        setTgTokenSet(cfg.bot_token_set)
        setTgChatIds(cfg.allowed_chat_ids.join('\n'))
        setTgNotifyCrash(cfg.notify_on_crash)
        setTgNotifyStart(cfg.notify_on_start)
        setTgNotifyStop(cfg.notify_on_stop)
        setTgNotifyRestart(cfg.notify_on_restart)
        setSettingsLoaded(true)
      })
      .catch(error => {
        setSettingsLoadError(error instanceof Error ? error.message : '读取 Telegram 配置失败')
      })
  }, [])

  function parseChatIds(): number[] {
    return tgChatIds
      .split('\n')
      .map(s => s.trim())
      .filter(Boolean)
      .map(Number)
      .filter(n => !isNaN(n) && n !== 0)
  }

  async function handleSaveTelegram(e: React.FormEvent) {
    e.preventDefault()
    setTgError(null)
    if (tgToken) {
      setTgError('请先验证新机器人令牌；验证通过后才会替换已保存令牌')
      return
    }
    setTgSaving(true)
    try {
      const payload: Parameters<typeof api.updateTelegramConfig>[0] = {
        enabled: tgEnabled,
        allowed_chat_ids: parseChatIds(),
        notify_on_crash: tgNotifyCrash,
        notify_on_start: tgNotifyStart,
        notify_on_stop: tgNotifyStop,
        notify_on_restart: tgNotifyRestart,
      }
      await api.updateTelegramConfig(payload)
      setTgSaved(true)
      setTgToken('')
      setTimeout(() => setTgSaved(false), 2000)
    } catch (err: unknown) {
      setTgError(err instanceof Error ? err.message : '保存 Telegram 配置失败')
    } finally {
      setTgSaving(false)
    }
  }

  async function handleValidateToken() {
    if (tgValidating || !tgToken) return
    const candidateToken = tgToken
    setTgValidating(true)
    setTgBotInfo(null)
    setTgError(null)
    try {
      const info = await api.getTelegramBotInfo(candidateToken)
      setTgBotInfo(info)
      if (info.ok) {
        await api.updateTelegramConfig({ bot_token: candidateToken })
        setTgTokenSet(true)
        setTgToken('')
        setTgChangingToken(false)
      }
    } catch (err: unknown) {
      setTgBotInfo({
        ok: false,
        username: null,
        first_name: null,
        error: err instanceof Error ? err.message : '请求失败',
      })
    } finally {
      setTgValidating(false)
    }
  }

  async function handleTestTelegram() {
    setTgTesting(true)
    setTgTestResult(null)
    try {
      await api.testTelegram()
      setTgTestResult('✅ 测试消息已发送！')
    } catch (err: unknown) {
      setTgTestResult(`❌ ${err instanceof Error ? err.message : '发送测试消息失败'}`)
    } finally {
      setTgTesting(false)
      setTimeout(() => setTgTestResult(null), 4000)
    }
  }

  if (settingsLoadError) {
    return (
      <div role="alert" style={{ color: 'var(--color-destructive)', padding: 16 }}>
        Telegram 配置加载失败，已禁止保存默认值：{settingsLoadError}
      </div>
    )
  }
  if (!settingsLoaded) {
    return <div style={{ padding: 16, color: 'var(--color-muted-foreground)' }}>加载中…</div>
  }

  return (
    <>
      <p style={sectionTitle}>Telegram 机器人</p>
      <div style={card}>
        <SettingRow
          label="启用 Telegram 机器人"
          description="允许通过 Telegram 控制进程并接收告警"
          isLast
          control={<Toggle checked={tgEnabled} onChange={v => setTgEnabled(v)} />}
        />
      </div>

      <p style={sectionTitle}>机器人令牌</p>
      <div style={card}>
        <SettingRow
          label="机器人令牌"
          description={
            tgTokenSet && !tgChangingToken
              ? '令牌已保存，点击“更换”进行替换'
              : '从 Telegram 上的 @BotFather 获取令牌'
          }
          isLast
          control={
            tgTokenSet && !tgChangingToken ? (
              <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <div
                  style={{
                    ...inputStyle,
                    width: 240,
                    fontSize: 12,
                    display: 'flex',
                    alignItems: 'center',
                    gap: 6,
                    color: 'var(--color-muted-foreground)',
                    background: 'var(--color-secondary)',
                  }}
                >
                  <span style={{ color: 'var(--color-status-running)', fontSize: 13 }}>✓</span>
                  <span style={{ fontFamily: 'monospace' }}>{tgTokenHint ?? '••••••••'}</span>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    setTgChangingToken(true)
                    setTgBotInfo(null)
                  }}
                  style={{
                    padding: '5px 12px',
                    fontSize: 12,
                    fontWeight: 500,
                    background: 'var(--color-card)',
                    color: 'var(--color-foreground)',
                    border: '1px solid var(--color-border)',
                    borderRadius: 5,
                    cursor: 'pointer',
                  }}
                >
                  更换
                </button>
              </div>
            ) : (
              <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <input
                  aria-label="Telegram 机器人令牌"
                  type="password"
                  placeholder="粘贴新的机器人令牌…"
                  value={tgToken}
                  disabled={tgValidating}
                  onChange={e => {
                    setTgToken(e.target.value)
                    setTgBotInfo(null)
                  }}
                  style={{ ...inputStyle, width: 240, fontSize: 12 }}
                  autoFocus
                />
                <button
                  type="button"
                  onClick={handleValidateToken}
                  disabled={tgValidating || !tgToken}
                  style={{
                    padding: '5px 12px',
                    fontSize: 12,
                    fontWeight: 500,
                    background: 'var(--color-card)',
                    color: 'var(--color-foreground)',
                    border: '1px solid var(--color-border)',
                    borderRadius: 5,
                    cursor: 'pointer',
                    opacity: tgValidating || !tgToken ? 0.5 : 1,
                  }}
                >
                  {tgValidating ? '检查中…' : '验证'}
                </button>
                {tgTokenSet && (
                  <button
                    type="button"
                    disabled={tgValidating}
                    onClick={() => {
                      setTgChangingToken(false)
                      setTgToken('')
                      setTgBotInfo(null)
                    }}
                    style={{
                      padding: '5px 10px',
                      fontSize: 12,
                      background: 'none',
                      color: 'var(--color-muted-foreground)',
                      border: '1px solid var(--color-border)',
                      borderRadius: 5,
                      cursor: tgValidating ? 'wait' : 'pointer',
                      opacity: tgValidating ? 0.5 : 1,
                    }}
                  >
                    取消
                  </button>
                )}
              </div>
            )
          }
        />
        {tgBotInfo && (
          <div
            role={tgBotInfo.ok ? 'status' : 'alert'}
            style={{
              marginTop: 8,
              padding: '8px 12px',
              borderRadius: 6,
              fontSize: 12,
              background: tgBotInfo.ok ? 'rgba(34,197,94,0.1)' : 'rgba(239,68,68,0.1)',
              color: tgBotInfo.ok ? 'var(--color-status-running)' : 'var(--color-status-errored)',
              border: `1px solid ${tgBotInfo.ok ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
            }}
          >
            {tgBotInfo.ok
              ? `✅ 已连接为 @${tgBotInfo.username ?? tgBotInfo.first_name}`
              : `❌ ${tgBotInfo.error ?? '令牌无效'}`}
          </div>
        )}
      </div>

      <p style={sectionTitle}>允许的聊天 ID</p>
      <div style={card}>
        <SettingRow
          label="允许的聊天 ID"
          description="只有这些 Telegram 用户/群组 ID 可以发送命令。每行一个 ID。向 @userinfobot 发送消息即可获取你的 ID。"
          isLast
          control={
            <textarea
              aria-label="允许的 Telegram 聊天 ID"
              placeholder={'123456789\n-987654321'}
              value={tgChatIds}
              onChange={e => setTgChatIds(e.target.value)}
              rows={4}
              style={{
                ...inputStyle,
                width: 200,
                resize: 'vertical',
                fontFamily: 'monospace',
                fontSize: 12,
              }}
            />
          }
        />
      </div>

      <p style={sectionTitle}>通知</p>
      <div style={card}>
        <SettingRow
          label="崩溃时通知"
          description="进程崩溃时发送消息"
          control={<Toggle checked={tgNotifyCrash} onChange={setTgNotifyCrash} />}
        />
        <SettingRow
          label="启动时通知"
          description="进程启动时发送消息"
          control={<Toggle checked={tgNotifyStart} onChange={setTgNotifyStart} />}
        />
        <SettingRow
          label="停止时通知"
          description="进程停止时发送消息"
          control={<Toggle checked={tgNotifyStop} onChange={setTgNotifyStop} />}
        />
        <SettingRow
          label="重启时通知"
          description="进程自动重启时发送消息"
          isLast
          control={<Toggle checked={tgNotifyRestart} onChange={setTgNotifyRestart} />}
        />
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 4 }}>
        <button
          onClick={handleSaveTelegram}
          disabled={tgSaving}
          style={{
            padding: '7px 18px',
            fontSize: 13,
            fontWeight: 500,
            background: tgSaved ? 'var(--color-status-running)' : 'var(--color-primary)',
            color: '#fff',
            border: 'none',
            borderRadius: 6,
            cursor: 'pointer',
            opacity: tgSaving ? 0.6 : 1,
            transition: 'background 0.2s',
          }}
        >
          {tgSaved ? '已保存！' : tgSaving ? '保存中…' : '保存'}
        </button>
        <button
          type="button"
          onClick={handleTestTelegram}
          disabled={tgTesting || !tgTokenSet || parseChatIds().length === 0}
          style={{
            padding: '7px 18px',
            fontSize: 13,
            fontWeight: 500,
            background: 'var(--color-card)',
            color: 'var(--color-foreground)',
            border: '1px solid var(--color-border)',
            borderRadius: 6,
            cursor: 'pointer',
            opacity: tgTesting || !tgTokenSet || parseChatIds().length === 0 ? 0.5 : 1,
          }}
        >
          {tgTesting ? '发送中…' : '发送测试消息'}
        </button>
        {tgTestResult && (
          <span
            role={tgTestResult.startsWith('✅') ? 'status' : 'alert'}
            style={{
              fontSize: 12,
              color: tgTestResult.startsWith('✅')
                ? 'var(--color-status-running)'
                : 'var(--color-status-errored)',
            }}
          >
            {tgTestResult}
          </span>
        )}
      </div>
      {tgError && (
        <p
          role="alert"
          style={{ fontSize: 12, color: 'var(--color-status-errored)', marginTop: 8 }}
        >
          {tgError}
        </p>
      )}

      <div
        style={{
          ...card,
          marginTop: 20,
          background: 'rgba(var(--color-primary-rgb, 99,102,241),0.05)',
          borderColor: 'rgba(var(--color-primary-rgb, 99,102,241),0.2)',
        }}
      >
        <p style={{ ...sectionTitle, color: 'var(--color-primary)', marginBottom: 8 }}>设置指南</p>
        <ol
          style={{
            fontSize: 12,
            color: 'var(--color-muted-foreground)',
            paddingLeft: 20,
            margin: 0,
            lineHeight: 1.8,
          }}
        >
          <li>
            在 Telegram 向 <strong>@BotFather</strong> 发送消息 → <code>/newbot</code> →
            复制上面的令牌
          </li>
          <li>
            点击<strong>验证</strong>确认令牌有效
          </li>
          <li>
            向你的机器人发送消息，再向 <strong>@userinfobot</strong> 发送消息以获取聊天 ID
          </li>
          <li>将聊天 ID 添加到“允许的聊天 ID”列表</li>
          <li>启用机器人并保存</li>
          <li>
            向机器人发送 <strong>/help</strong> 查看可用命令
          </li>
        </ol>
        <p
          style={{
            fontSize: 12,
            color: 'var(--color-muted-foreground)',
            marginTop: 12,
            marginBottom: 0,
          }}
        >
          <strong>命令：</strong> /list · /start &lt;name&gt; · /stop &lt;name&gt; · /restart
          &lt;name&gt; · /logs &lt;name&gt; [lines] · /status &lt;name&gt; · /ping · /help
        </p>
      </div>
    </>
  )
}
