const blockedSchemes = new Set(['http', 'https', 'file', 'javascript', 'data'])

export function isDesktopLaunchUri(value: string | null | undefined): value is string {
  if (!value || value.length > 512 || !/^[\x21-\x7e]+$/.test(value)) return false
  const match = /^([A-Za-z][A-Za-z0-9+.-]*):\/\/([^/?#]+)(?:[/?#].*)?$/.exec(value)
  if (!match || blockedSchemes.has(match[1].toLowerCase()) || match[2].includes('@')) return false
  return !/[\\"'<>`]/.test(value)
}
