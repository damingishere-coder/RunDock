import { render, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import TunnelsPage from './TunnelsPage'

vi.mock('@/lib/api', () => ({
  api: {
    getTunnels: vi.fn(),
    getTunnelSettings: vi.fn(),
  },
}))

describe('TunnelsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getTunnels).mockResolvedValue({ tunnels: [] })
    vi.mocked(api.getTunnelSettings).mockResolvedValue({
      provider: 'cloudflare',
      cloudflare: { token: null },
      ngrok: { auth_token: null },
      custom: { binary_path: '', args_template: '' },
    })
  })

  it('loads the tunnel list once on mount even when no tunnel is starting', async () => {
    render(<TunnelsPage />)

    await waitFor(() => expect(api.getTunnels).toHaveBeenCalledTimes(1))
  })
})
