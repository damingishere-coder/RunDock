import { afterEach, describe, expect, it } from 'vitest'
import {
  getActiveServer,
  getServers,
  isLoopbackHost,
  normalizeServerHost,
  resolveActiveServer,
  serverBaseUrl,
  saveServers,
  type RemoteServer,
} from './servers'

afterEach(() => window.localStorage.clear())

function directServer(overrides: Partial<RemoteServer> = {}): RemoteServer {
  return {
    id: 'remote-1',
    name: 'Remote',
    host: 'example.test',
    port: 2999,
    connectionType: 'direct',
    ...overrides,
  }
}

describe('remote server transport', () => {
  it('defaults non-loopback direct connections to HTTPS', () => {
    expect(serverBaseUrl(directServer())).toBe('https://example.test:2999/api/v1')
  })

  it('allows HTTP only for loopback direct connections', () => {
    expect(serverBaseUrl(directServer({ host: '127.0.0.1', protocol: 'http' }))).toBe(
      'http://127.0.0.1:2999/api/v1'
    )
    expect(() => serverBaseUrl(directServer({ protocol: 'http' }))).toThrow('必须使用 HTTPS')
    expect(isLoopbackHost('[::1]')).toBe(true)
    expect(isLoopbackHost('127.255.255.255')).toBe(true)
    expect(isLoopbackHost('0127.0.0.1')).toBe(false)
    expect(isLoopbackHost('127.00.0.1')).toBe(false)
    expect(isLoopbackHost('127.999.999.999')).toBe(false)
    expect(() => serverBaseUrl(directServer({ host: '0127.0.0.1', protocol: 'http' }))).toThrow(
      '必须使用 HTTPS'
    )
    expect(() =>
      serverBaseUrl(directServer({ host: '127.999.999.999', protocol: 'http' }))
    ).toThrow('必须使用 HTTPS')
  })

  it('fails closed when the selected server no longer exists', () => {
    window.localStorage.setItem('alter_active_server', 'missing')
    window.localStorage.setItem('alter_servers', '[]')
    expect(() => getActiveServer()).toThrow('活动服务器配置已不存在')
  })

  it('provides a recoverable local shell state for invalid active storage', () => {
    window.localStorage.setItem('alter_active_server', 'missing')
    window.localStorage.setItem('alter_servers', '[]')
    const resolution = resolveActiveServer()
    expect(resolution.server.id).toBe('local')
    expect(resolution.error).toContain('活动服务器配置已不存在')
  })

  it('rejects corrupted or incorrectly shaped stored servers', () => {
    window.localStorage.setItem('alter_servers', '{broken')
    expect(() => getServers()).toThrow('服务器配置已损坏')
    window.localStorage.setItem('alter_servers', JSON.stringify({ id: 'not-an-array' }))
    expect(() => getServers()).toThrow('服务器配置格式无效')
    window.localStorage.setItem('alter_servers', JSON.stringify([{ id: 'incomplete' }]))
    expect(() => getServers()).toThrow('服务器配置格式无效')
  })

  it('rejects duplicate server IDs when loading or saving', () => {
    const duplicate = [directServer(), directServer({ host: 'other.example.test' })]
    expect(() => saveServers(duplicate)).toThrow('无效的服务器配置')
    window.localStorage.setItem('alter_servers', JSON.stringify(duplicate))
    expect(() => getServers()).toThrow('服务器配置格式无效')
  })

  it('normalizes valid hosts and rejects URL-shaped or malformed input', () => {
    expect(normalizeServerHost(' Example.Test ')).toBe('example.test')
    expect(normalizeServerHost('[::1]')).toBe('::1')
    expect(normalizeServerHost('https://example.test/path')).toBeNull()
    expect(normalizeServerHost('bad host')).toBeNull()
    expect(normalizeServerHost('-bad.example')).toBeNull()
  })
})
