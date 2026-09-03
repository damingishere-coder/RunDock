// @group Utilities : Shared primitives for settings tab components

import { createContext, useContext, useId, useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { descStyle, labelStyle, lastRowStyle, rowStyle } from './sharedStyles'

const SettingLabelContext = createContext<string | null>(null)

// @group Utilities > Toggle : iOS-style toggle switch
export function Toggle({
  checked,
  onChange,
}: {
  checked: boolean
  onChange: (v: boolean) => void
}) {
  const accessibleLabel = useContext(SettingLabelContext)
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={accessibleLabel ?? '切换设置'}
      onClick={() => onChange(!checked)}
      style={{
        width: 40,
        height: 22,
        borderRadius: 11,
        border: 'none',
        cursor: 'pointer',
        background: checked ? 'var(--color-primary)' : 'var(--color-border)',
        position: 'relative',
        transition: 'background 0.2s',
        flexShrink: 0,
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 3,
          left: checked ? 20 : 3,
          width: 16,
          height: 16,
          borderRadius: '50%',
          background: '#fff',
          transition: 'left 0.2s',
          boxShadow: '0 1px 3px rgba(0,0,0,0.4)',
        }}
      />
    </button>
  )
}

// @group Utilities > SettingRow : A single setting row with label, description, and control
export function SettingRow({
  label,
  description,
  control,
  isLast = false,
}: {
  label: string
  description?: React.ReactNode
  control: React.ReactNode
  isLast?: boolean
}) {
  const labelId = useId()
  return (
    <SettingLabelContext.Provider value={label}>
      <div style={isLast ? lastRowStyle : rowStyle}>
        <div style={{ flex: 1, paddingRight: 24 }}>
          <div id={labelId} style={labelStyle}>
            {label}
          </div>
          {description && <div style={descStyle}>{description}</div>}
        </div>
        <div role="group" aria-labelledby={labelId} style={{ flexShrink: 0 }}>
          {control}
        </div>
      </div>
    </SettingLabelContext.Provider>
  )
}

// @group Utilities > CopyPath : Path display field with one-click copy
export function CopyPath({ value }: { value: string }) {
  const [copied, setCopied] = useState(false)
  const [copyError, setCopyError] = useState(false)
  function copy() {
    setCopyError(false)
    navigator.clipboard
      .writeText(value)
      .then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 1800)
      })
      .catch(() => setCopyError(true))
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
      <code
        style={{
          fontSize: 11,
          fontFamily: 'monospace',
          background: 'var(--color-muted)',
          border: '1px solid var(--color-border)',
          borderRadius: 4,
          padding: '3px 8px',
          color: 'var(--color-foreground)',
          maxWidth: 340,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          display: 'block',
        }}
        title={value}
      >
        {value}
      </code>
      <button
        onClick={copy}
        title="复制路径"
        style={{
          padding: 4,
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          color: copied ? 'var(--color-status-running)' : 'var(--color-muted-foreground)',
          display: 'flex',
          alignItems: 'center',
        }}
      >
        {copied ? <Check size={13} /> : <Copy size={13} />}
      </button>
      {copyError && (
        <span role="alert" style={{ fontSize: 11, color: 'var(--color-destructive)' }}>
          复制失败，请手动复制
        </span>
      )}
    </div>
  )
}
