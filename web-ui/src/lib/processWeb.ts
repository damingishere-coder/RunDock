import { isLoopbackHost, type RemoteServer } from '@/lib/servers'

export interface PortScanEntry {
  pid: number | null
  port: number
  protocol: string
  local_address: string
  state: string
  ancestor_pids?: number[]
}

export interface ProjectWebTarget {
  port: number
  url: string
}

export function isPortScanEntries(value: unknown): value is PortScanEntry[] {
  return (
    Array.isArray(value) &&
    value.every(entry => {
      if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) return false
      const port = entry as Record<string, unknown>
      return (
        (port.pid === null || (Number.isInteger(port.pid) && (port.pid as number) >= 0)) &&
        Number.isInteger(port.port) &&
        (port.port as number) >= 1 &&
        (port.port as number) <= 65_535 &&
        typeof port.protocol === 'string' &&
        typeof port.local_address === 'string' &&
        typeof port.state === 'string' &&
        (port.ancestor_pids === undefined ||
          (Array.isArray(port.ancestor_pids) &&
            port.ancestor_pids.every(pid => Number.isInteger(pid) && pid > 0)))
      )
    })
  )
}

export function isListeningTcpPort(entry: PortScanEntry): boolean {
  if (typeof entry.protocol !== 'string' || typeof entry.state !== 'string') return false
  if (entry.protocol.toUpperCase() !== 'TCP') return false
  const state = entry.state.toUpperCase()
  return state === 'LISTEN' || state === 'LISTENING'
}

export function listeningPortsByManagedPid(
  entries: PortScanEntry[],
  managedPids: Iterable<number>
): Map<number, number[]> {
  const managed = new Set(managedPids)
  const result = new Map<number, number[]>()

  for (const entry of entries) {
    if (!entry.pid || entry.pid <= 0 || !isListeningTcpPort(entry)) continue
    const ownerPid = [entry.pid, ...(entry.ancestor_pids ?? [])].find(pid => managed.has(pid))
    if (ownerPid == null) continue

    const ports = result.get(ownerPid) ?? []
    if (!ports.includes(entry.port)) ports.push(entry.port)
    result.set(ownerPid, ports)
  }

  result.forEach(ports => ports.sort((a, b) => a - b))
  return result
}

export function projectWebUrl(port: number, server: RemoteServer): string | null {
  // An SSH RunDock connection forwards only the daemon port. Opening a project
  // port on 127.0.0.1 would therefore point at the wrong machine.
  if (server.connectionType === 'ssh') return null

  const host = server.id === 'local' ? '127.0.0.1' : server.host
  const protocol =
    server.id === 'local'
      ? 'http'
      : (server.protocol ?? (isLoopbackHost(server.host) ? 'http' : 'https'))
  if (protocol === 'http' && !isLoopbackHost(host)) return null
  const urlHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host
  return `${protocol}://${urlHost}:${port}/`
}

export function projectWebTargets(ports: number[], server: RemoteServer): ProjectWebTarget[] {
  return ports.flatMap(port => {
    const url = projectWebUrl(port, server)
    return url ? [{ port, url }] : []
  })
}
