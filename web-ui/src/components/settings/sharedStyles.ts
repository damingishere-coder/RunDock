// @group Utilities : Shared style tokens for settings tabs

export const sectionTitle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 700,
  letterSpacing: '0.08em',
  color: 'var(--color-muted-foreground)',
  textTransform: 'uppercase',
  marginBottom: 12,
  marginTop: 0,
}

export const card: React.CSSProperties = {
  background: 'var(--color-card)',
  border: '1px solid var(--color-border)',
  borderRadius: 8,
  padding: '18px 20px',
  marginBottom: 16,
}

export const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  padding: '10px 0',
  borderBottom: '1px solid var(--color-border)',
}

export const lastRowStyle: React.CSSProperties = {
  ...rowStyle,
  borderBottom: 'none',
  paddingBottom: 0,
}

export const labelStyle: React.CSSProperties = {
  fontSize: 13,
  fontWeight: 500,
  color: 'var(--color-foreground)',
}

export const descStyle: React.CSSProperties = {
  fontSize: 11,
  color: 'var(--color-muted-foreground)',
  marginTop: 2,
}

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

export const selectStyle: React.CSSProperties = {
  ...inputStyle,
  width: 'auto',
  minWidth: 130,
  fontSize: 12,
  padding: '5px 10px',
  cursor: 'pointer',
}
