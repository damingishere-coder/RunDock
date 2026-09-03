export function terminalHistoryKey(name: string | undefined, cwd: string): string | undefined {
  if (!name && !cwd) return undefined
  const source = `${name ?? ''}\0${cwd}`
  let first = 0x811c9dc5
  let second = 0x9e3779b9
  for (let index = 0; index < source.length; index += 1) {
    const code = source.charCodeAt(index)
    first = Math.imul(first ^ code, 0x01000193)
    second = Math.imul(second ^ code, 0x85ebca6b)
  }
  const digest = `${(first >>> 0).toString(16).padStart(8, '0')}${(second >>> 0)
    .toString(16)
    .padStart(8, '0')}`
  return `${name ? 'proc' : 'cwd'}:${digest}`
}

export interface TerminalHistoryEntry {
  cmd: string
  count: number
}

export function mergeTerminalHistory<T extends TerminalHistoryEntry>(
  current: T[],
  incoming: T[],
  limit = 150
): T[] {
  const merged = current.map(entry => ({ ...entry })) as T[]
  const positions = new Map(merged.map((entry, index) => [entry.cmd, index]))
  for (const entry of incoming) {
    const position = positions.get(entry.cmd)
    if (position === undefined) {
      positions.set(entry.cmd, merged.length)
      merged.push({ ...entry } as T)
    } else {
      merged[position].count = Math.max(merged[position].count, entry.count)
    }
  }
  return merged.slice(0, Math.max(0, limit))
}
