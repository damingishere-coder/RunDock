export type InstallStreamEvent =
  | { done: true; ok: boolean; line?: never }
  | { line: string; done?: never; ok?: never }

export function parseInstallStreamEvent(raw: string): InstallStreamEvent | null {
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    return null
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const record = value as Record<string, unknown>
  if (record.done === true && typeof record.ok === 'boolean') {
    return { done: true, ok: record.ok }
  }
  if (typeof record.line === 'string') return { line: record.line }
  return null
}
