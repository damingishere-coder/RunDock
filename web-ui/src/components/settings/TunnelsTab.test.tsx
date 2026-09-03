import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TunnelsTab from './TunnelsTab'

const apiMocks = vi.hoisted(() => ({
  getTunnelSettings: vi.fn(),
  streamInstallProvider: vi.fn(),
  testTunnelProvider: vi.fn(),
  updateTunnelSettings: vi.fn(),
}))

vi.mock('@/lib/api', () => ({ api: apiMocks }))

describe('TunnelsTab installation', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.getTunnelSettings.mockResolvedValue({
      provider: 'cloudflare',
      cloudflare: { token: null },
      ngrok: { auth_token: null },
      custom: { binary_path: '', args_template: '' },
    })
  })

  it('cancels a pending stream-ticket request before EventSource exists', async () => {
    let requestSignal: AbortSignal | undefined
    apiMocks.streamInstallProvider.mockImplementation(
      (_provider: string, init?: RequestInit) =>
        new Promise((_resolve, reject) => {
          requestSignal = init?.signal ?? undefined
          requestSignal?.addEventListener('abort', () =>
            reject(new DOMException('aborted', 'AbortError'))
          )
        })
    )

    render(<TunnelsTab />)
    const installButtons = await screen.findAllByRole('button', { name: '安装' })
    fireEvent.click(installButtons[0])
    const cancel = await screen.findByRole('button', { name: '取消等待' })
    expect(requestSignal?.aborted).toBe(false)

    fireEvent.click(cancel)

    await waitFor(() => expect(requestSignal?.aborted).toBe(true))
    expect(await screen.findByText('已取消等待安装输出。')).toBeInTheDocument()
  })
})
