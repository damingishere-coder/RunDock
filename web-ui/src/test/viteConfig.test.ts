import { describe, expect, it } from 'vitest'
import { DEV_SERVER_HOST, DEV_SERVER_PROXY_TARGET } from '../lib/devServerPolicy'

describe('Vite development boundary', () => {
  it('keeps both the development listener and daemon proxy on loopback', () => {
    expect(DEV_SERVER_HOST).toBe('127.0.0.1')
    expect(new URL(DEV_SERVER_PROXY_TARGET).hostname).toBe('127.0.0.1')
  })
})
