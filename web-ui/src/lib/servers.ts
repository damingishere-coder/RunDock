// @group Configuration : Remote server store — manages local + remote alter daemon connections

// @group Types > Server : Connection mode for a remote server
export type ConnectionType = 'direct' | 'ssh'

// @group Types > Server : A registered alter-pm2 server (local or remote)
export interface RemoteServer {
  id: string
  name: string
  /** For direct: daemon host. For ssh: always '127.0.0.1' (local tunnel endpoint). */
  host: string
  /** For direct: daemon port. For ssh: local forwarded port (e.g. 3001). */
  port: number
  connectionType: ConnectionType
  /** Direct connections use HTTPS unless the host is loopback. */
  protocol?: 'http' | 'https'
  // SSH-specific fields (only when connectionType === 'ssh')
  sshHost?: string // remote machine hostname/IP
  sshPort?: number // SSH server port (default 22)
  sshUser?: string // SSH username
  sshKeyPath?: string // path to private key, e.g. ~/.ssh/id_rsa (optional)
  remoteDaemonPort?: number // daemon port on the remote machine (default 2999)
}

const LOCAL_ID = 'local'
const SERVERS_KEY = 'alter_servers'
const ACTIVE_KEY = 'alter_active_server'

// @group Configuration > Server : Built-in local server — always present, cannot be removed
export const LOCAL_SERVER: RemoteServer = {
  id: LOCAL_ID,
  name: '本地',
  host: '127.0.0.1',
  port: 2999,
  connectionType: 'direct',
}

// @group Configuration > Server : Load remote servers from localStorage
export function getServers(): RemoteServer[] {
  const raw = localStorage.getItem(SERVERS_KEY)
  if (!raw) return []
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    throw new Error('服务器配置已损坏，请在浏览器存储中删除 alter_servers 后重新添加')
  }
  if (
    !Array.isArray(parsed) ||
    parsed.length > 100 ||
    !parsed.every(isRemoteServer) ||
    new Set(parsed.map(server => server.id)).size !== parsed.length
  ) {
    throw new Error('服务器配置格式无效，请删除损坏的服务器配置后重新添加')
  }
  return parsed
}

function optionalString(value: unknown, maxLength: number): boolean {
  return value === undefined || (typeof value === 'string' && value.length <= maxLength)
}

function optionalPort(value: unknown): boolean {
  return (
    value === undefined ||
    (Number.isInteger(value) && (value as number) >= 1 && (value as number) <= 65535)
  )
}

export function normalizeServerHost(value: string): string | null {
  const trimmed = value.trim()
  if (
    !trimmed ||
    trimmed.length > 255 ||
    /[\s/?#@]/.test(trimmed) ||
    [...trimmed].some(character => {
      const code = character.charCodeAt(0)
      return code <= 31 || code === 127
    }) ||
    trimmed.includes('://')
  )
    return null
  const host = trimmed.startsWith('[') && trimmed.endsWith(']') ? trimmed.slice(1, -1) : trimmed
  if (host.includes(':')) {
    try {
      const parsed = new URL(`https://[${host}]:443`)
      return parsed.hostname.replace(/^\[|\]$/g, '').toLowerCase()
    } catch {
      return null
    }
  }
  if (
    host.length > 253 ||
    !host
      .split('.')
      .every(
        label =>
          label.length >= 1 &&
          label.length <= 63 &&
          /^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/.test(label)
      )
  )
    return null
  return host.toLowerCase()
}

function isRemoteServer(value: unknown): value is RemoteServer {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false
  const server = value as Record<string, unknown>
  return (
    typeof server.id === 'string' &&
    server.id.length > 0 &&
    server.id.length <= 128 &&
    server.id !== LOCAL_ID &&
    typeof server.name === 'string' &&
    server.name.length > 0 &&
    server.name.length <= 128 &&
    typeof server.host === 'string' &&
    normalizeServerHost(server.host) !== null &&
    optionalPort(server.port) &&
    server.port !== undefined &&
    (server.connectionType === 'direct' || server.connectionType === 'ssh') &&
    (server.protocol === undefined || server.protocol === 'http' || server.protocol === 'https') &&
    optionalString(server.sshHost, 255) &&
    optionalPort(server.sshPort) &&
    optionalString(server.sshUser, 128) &&
    optionalString(server.sshKeyPath, 1_024) &&
    optionalPort(server.remoteDaemonPort) &&
    (server.connectionType !== 'ssh' ||
      (typeof server.sshHost === 'string' && normalizeServerHost(server.sshHost) !== null))
  )
}

// @group Configuration > Server : Persist remote servers to localStorage
export function saveServers(servers: RemoteServer[]): void {
  if (
    servers.length > 100 ||
    !servers.every(isRemoteServer) ||
    new Set(servers.map(server => server.id)).size !== servers.length
  ) {
    throw new Error('拒绝保存无效的服务器配置')
  }
  localStorage.setItem(SERVERS_KEY, JSON.stringify(servers))
}

// @group Configuration > Server : Get the active server ID (defaults to 'local')
export function getActiveServerId(): string {
  return localStorage.getItem(ACTIVE_KEY) ?? LOCAL_ID
}

// @group Configuration > Server : Set the active server ID
export function setActiveServerId(id: string): void {
  localStorage.setItem(ACTIVE_KEY, id)
}

export function resetServers(): void {
  localStorage.removeItem(SERVERS_KEY)
  localStorage.removeItem(ACTIVE_KEY)
}

// @group Configuration > Server : Resolve the active server object
export function getActiveServer(): RemoteServer {
  const id = getActiveServerId()
  if (id === LOCAL_ID) return LOCAL_SERVER
  const remotes = getServers()
  const active = remotes.find(s => s.id === id)
  if (!active) throw new Error('当前活动服务器配置已不存在，请重新选择服务器')
  return active
}

export function resolveActiveServer(): { server: RemoteServer; error: string | null } {
  try {
    return { server: getActiveServer(), error: null }
  } catch (error) {
    return {
      server: LOCAL_SERVER,
      error: error instanceof Error ? error.message : '无法读取活动服务器配置',
    }
  }
}

export function isLoopbackHost(host: string): boolean {
  const normalized = host
    .trim()
    .toLowerCase()
    .replace(/^\[|\]$/g, '')
  if (normalized === 'localhost' || normalized === '::1') return true

  const octets = normalized.split('.')
  return (
    octets.length === 4 &&
    octets[0] === '127' &&
    octets.every(octet => /^(?:0|[1-9]\d{0,2})$/.test(octet) && Number(octet) <= 255)
  )
}

// @group Configuration > Server : Build the API base URL for a server
export function serverBaseUrl(server: RemoteServer): string {
  if (server.id === LOCAL_ID) return '/api/v1'
  if (server.connectionType === 'ssh') {
    // SSH tunnel: connect to the locally-forwarded port on localhost
    return `http://127.0.0.1:${server.port}/api/v1`
  }
  const protocol = server.protocol ?? (isLoopbackHost(server.host) ? 'http' : 'https')
  if (protocol === 'http' && !isLoopbackHost(server.host)) {
    throw new Error('非本机直连必须使用 HTTPS；如服务端未配置 TLS，请使用 SSH 隧道')
  }
  const host =
    server.host.includes(':') && !server.host.startsWith('[') ? `[${server.host}]` : server.host
  return `${protocol}://${host}:${server.port}/api/v1`
}

// @group Configuration > Server : localStorage key for a server's session token
export function serverTokenKey(server: RemoteServer): string {
  return server.id === LOCAL_ID ? 'alter_session_token' : `alter_session_${server.id}`
}

// @group Utilities > Server : Build the SSH tunnel command string for an SSH-type server
export function sshTunnelCommand(server: RemoteServer): string {
  const localPort = server.port
  const remotePort = server.remoteDaemonPort ?? 2999
  const sshHost = server.sshHost ?? ''
  const sshPort = server.sshPort ?? 22
  const user = server.sshUser ? `${server.sshUser}@` : ''
  const keyFlag = server.sshKeyPath ? ` -i "${server.sshKeyPath}"` : ''
  const portFlag = sshPort !== 22 ? ` -p ${sshPort}` : ''
  return `ssh -L ${localPort}:localhost:${remotePort}${keyFlag}${portFlag} -N ${user}${sshHost}`
}
