import { describe, expect, it } from 'vitest'
import {
  isListeningPortState,
  isListeningTcpPort,
  isPortScanEntries,
  listeningPortsByManagedPid,
  projectWebTargets,
  projectWebUrl,
} from './processWeb'
import type { RemoteServer } from './servers'

describe('isListeningPortState', () => {
  it('normalizes Unix and Windows listener states', () => {
    expect(isListeningPortState('LISTEN')).toBe(true)
    expect(isListeningPortState('listening')).toBe(true)
    expect(isListeningPortState('ESTABLISHED')).toBe(false)
  })
})

const localServer: RemoteServer = {
  id: 'local',
  name: '本地',
  host: '127.0.0.1',
  port: 2999,
  connectionType: 'direct',
}

describe('project web links', () => {
  it('rejects malformed port scan payloads before list pages render them', () => {
    expect(isPortScanEntries([{ pid: 4, port: '2999', protocol: 'TCP', state: 'LISTEN' }])).toBe(
      false
    )
    expect(
      isPortScanEntries([
        {
          pid: 4,
          port: 2999,
          protocol: 'TCP',
          local_address: '127.0.0.1',
          state: 'LISTEN',
          ancestor_pids: [1, 2],
        },
      ])
    ).toBe(true)
  })

  it('accepts the Windows TIME_WAIT PID 0 sentinel without assigning it to a project', () => {
    const timeWait = {
      pid: 0,
      port: 2999,
      protocol: 'TCP',
      local_address: '127.0.0.1:2999',
      state: 'TIME_WAIT',
      ancestor_pids: [],
    }

    expect(isPortScanEntries([timeWait])).toBe(true)
    expect(isPortScanEntries([{ ...timeWait, pid: -1 }])).toBe(false)
    expect(isPortScanEntries([{ ...timeWait, pid: 0.5 }])).toBe(false)
    expect(isPortScanEntries([{ ...timeWait, pid: '0' }])).toBe(false)
    expect(listeningPortsByManagedPid([timeWait], [0])).toEqual(new Map())
    expect(listeningPortsByManagedPid([{ ...timeWait, state: 'LISTEN' }], [0])).toEqual(new Map())
  })

  it('keeps only TCP listeners as web candidates', () => {
    expect(
      isListeningTcpPort({
        pid: 1,
        port: 5173,
        protocol: 'TCP',
        local_address: '127.0.0.1:5173',
        state: 'LISTENING',
      })
    ).toBe(true)
    expect(
      isListeningTcpPort({
        pid: 1,
        port: 5173,
        protocol: 'tcp',
        local_address: '127.0.0.1:5173',
        state: 'LISTEN',
      })
    ).toBe(true)
    expect(
      isListeningTcpPort({
        pid: 1,
        port: 53,
        protocol: 'UDP',
        local_address: '0.0.0.0:53',
        state: '',
      })
    ).toBe(false)
    expect(
      isListeningTcpPort({
        pid: 1,
        port: 5173,
        protocol: 'TCP',
        local_address: '127.0.0.1:5173',
        state: 'ESTABLISHED',
      })
    ).toBe(false)
  })

  it('builds local HTTP and direct-remote HTTPS URLs', () => {
    expect(projectWebUrl(5173, localServer)).toBe('http://127.0.0.1:5173/')
    expect(
      projectWebUrl(8080, {
        ...localServer,
        id: 'remote',
        host: '192.0.2.10',
        protocol: 'https',
      })
    ).toBe('https://192.0.2.10:8080/')
    expect(
      projectWebUrl(8080, {
        ...localServer,
        id: 'insecure',
        host: '192.0.2.10',
        protocol: 'http',
      })
    ).toBeNull()
    expect(projectWebUrl(8080, { ...localServer, id: 'default-secure', host: 'example.com' })).toBe(
      'https://example.com:8080/'
    )
    expect(projectWebUrl(8080, { ...localServer, id: 'ipv6', host: '::1' })).toBe(
      'http://[::1]:8080/'
    )
  })

  it('does not generate misleading links for SSH-only daemon tunnels', () => {
    const sshServer: RemoteServer = {
      ...localServer,
      id: 'ssh',
      connectionType: 'ssh',
      sshHost: 'example.invalid',
    }
    expect(projectWebUrl(5173, sshServer)).toBeNull()
    expect(projectWebTargets([5173, 8766], sshServer)).toEqual([])
  })

  it('returns one target per detected port for the multi-port menu', () => {
    expect(projectWebTargets([5173, 8766], localServer)).toEqual([
      { port: 5173, url: 'http://127.0.0.1:5173/' },
      { port: 8766, url: 'http://127.0.0.1:8766/' },
    ])
  })

  it('assigns child listeners to managed ancestors and deduplicates sorted ports', () => {
    const result = listeningPortsByManagedPid(
      [
        {
          pid: 501,
          port: 8766,
          protocol: 'TCP',
          local_address: '127.0.0.1:8766',
          state: 'LISTENING',
          ancestor_pids: [101],
        },
        { pid: 101, port: 5173, protocol: 'tcp', local_address: '127.0.0.1:5173', state: 'LISTEN' },
        {
          pid: 501,
          port: 5173,
          protocol: 'TCP',
          local_address: '127.0.0.1:5173',
          state: 'LISTENING',
          ancestor_pids: [101],
        },
        { pid: 101, port: 53, protocol: 'UDP', local_address: '127.0.0.1:53', state: 'LISTENING' },
        {
          pid: 999,
          port: 9000,
          protocol: 'TCP',
          local_address: '127.0.0.1:9000',
          state: 'LISTENING',
        },
      ],
      [101]
    )

    expect(result.get(101)).toEqual([5173, 8766])
    expect(result.has(999)).toBe(false)
  })
})
