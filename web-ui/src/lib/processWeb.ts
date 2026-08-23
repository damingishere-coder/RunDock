import type { RemoteServer } from '@/lib/servers'

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

export function isListeningTcpPort(entry: PortScanEntry): boolean {
  if (entry.protocol.toUpperCase() !== 'TCP') return false
  const state = entry.state.toUpperCase()
  return state === 'LISTEN' || state === 'LISTENING'
}

export function listeningPortsByManagedPid(
  entries: PortScanEntry[],
  managedPids: Iterable<number>,
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
  // An SSH Alter connection forwards only the daemon port. Opening a project
  // port on 127.0.0.1 would therefore point at the wrong machine.
  if (server.connectionType === 'ssh') return null

  const host = server.id === 'local' ? '127.0.0.1' : server.host
  const urlHost = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host
  return `http://${urlHost}:${port}/`
}

export function projectWebTargets(ports: number[], server: RemoteServer): ProjectWebTarget[] {
  return ports.flatMap(port => {
    const url = projectWebUrl(port, server)
    return url ? [{ port, url }] : []
  })
}
