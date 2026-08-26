// @group Utilities : Shared form layout primitives

import type { ReactNode } from 'react'

export function FormCard({
  children,
  onSubmit,
}: {
  children: ReactNode
  onSubmit: (e: React.FormEvent) => void
}) {
  return (
    <form
      onSubmit={onSubmit}
      style={{
        background: 'var(--color-card)',
        border: '1px solid var(--color-border)',
        borderRadius: 8,
        padding: '20px 24px',
        display: 'flex',
        flexDirection: 'column',
        gap: 14,
        maxWidth: 860,
      }}
    >
      {children}
    </form>
  )
}

export function FormRow({ children }: { children: ReactNode }) {
  return <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>{children}</div>
}

export function FormField({
  label,
  children,
  associate = true,
}: {
  label: ReactNode
  children: ReactNode
  associate?: boolean
}) {
  const labelStyle = {
    fontSize: 12,
    fontWeight: 500,
    color: 'var(--color-muted-foreground)',
  } as const
  if (associate) {
    return (
      <label style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
        <span style={labelStyle}>{label}</span>
        {children}
      </label>
    )
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
      <div style={labelStyle}>{label}</div>
      {children}
    </div>
  )
}
