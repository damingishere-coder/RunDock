// @group Utilities : Shared styles for process and cron forms

export const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '6px 10px',
  fontSize: 13,
  background: 'var(--color-input)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  color: 'var(--color-foreground)',
  outline: 'none',
}

export const primaryBtnStyle: React.CSSProperties = {
  padding: '7px 20px',
  fontSize: 13,
  fontWeight: 600,
  background: 'var(--color-primary)',
  border: 'none',
  borderRadius: 5,
  cursor: 'pointer',
  color: '#fff',
}

export const browseBtnStyle: React.CSSProperties = {
  padding: '0 10px',
  flexShrink: 0,
  height: '100%',
  minHeight: 32,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  cursor: 'pointer',
  color: 'var(--color-muted-foreground)',
}
