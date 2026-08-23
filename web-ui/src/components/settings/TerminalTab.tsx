// @group BusinessLogic > TerminalTab : Terminal keyboard shortcut settings

import type { AppSettings } from '@/lib/settings'
import { card, descStyle, inputStyle, labelStyle, lastRowStyle, rowStyle, sectionTitle } from './shared'

interface Props {
  settings: AppSettings
  onUpdate: (patch: Partial<AppSettings>) => void
}

// @group BusinessLogic > TerminalTab : Shortcut config rows
export default function TerminalTab({ settings, onUpdate }: Props) {
  const s = settings.terminalShortcuts

  function update(key: keyof AppSettings['terminalShortcuts'], value: string) {
    onUpdate({ terminalShortcuts: { ...s, [key]: value } })
  }

  return (
    <div>
      <p style={sectionTitle}>键盘快捷键</p>
      <div style={card}>

        <div style={rowStyle}>
          <div style={{ flex: 1, paddingRight: 24 }}>
            <div style={labelStyle}>拆分窗格</div>
            <div style={descStyle}>将当前终端拆分为左右并列的两个窗格</div>
          </div>
          <ShortcutInput value={s.splitPane} onChange={v => update('splitPane', v)} />
        </div>

        <div style={rowStyle}>
          <div style={{ flex: 1, paddingRight: 24 }}>
            <div style={labelStyle}>复制标签页</div>
            <div style={descStyle}>使用当前标签页的相同工作目录打开新标签页</div>
          </div>
          <ShortcutInput value={s.duplicateTab} onChange={v => update('duplicateTab', v)} />
        </div>

        <div style={lastRowStyle}>
          <div style={{ flex: 1, paddingRight: 24 }}>
            <div style={labelStyle}>新建终端</div>
            <div style={descStyle}>打开空白终端标签页</div>
          </div>
          <ShortcutInput value={s.newTab} onChange={v => update('newTab', v)} />
        </div>

      </div>

      <p style={{ ...sectionTitle, marginTop: 8 }}>快捷键格式</p>
      <div style={card}>
        <div style={{ fontSize: 12, color: 'var(--color-muted-foreground)', lineHeight: 1.7 }}>
          使用 <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>+</code> 连接修饰键和按键
          <br />
          修饰键：<code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>ctrl</code>{' '}
          <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>shift</code>{' '}
          <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>alt</code>{' '}
          <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>meta</code>
          <br />
          示例：<code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>ctrl+shift+t</code>{' '}
          <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>alt+t</code>{' '}
          <code style={{ background: 'var(--color-muted)', padding: '1px 5px', borderRadius: 3 }}>ctrl+t</code>
        </div>
      </div>
    </div>
  )
}

// @group Utilities > ShortcutInput : Keyboard shortcut text input with monospace font
function ShortcutInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <input
      value={value}
      onChange={e => onChange(e.target.value.toLowerCase())}
      style={{
        ...inputStyle,
        width: 170,
        fontFamily: '"Cascadia Code", "Fira Code", Consolas, monospace',
        fontSize: 12,
      }}
      placeholder="例如：ctrl+shift+t"
      spellCheck={false}
    />
  )
}
