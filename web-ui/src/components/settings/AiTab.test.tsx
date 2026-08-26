import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AiTab from './AiTab'

const apiMocks = vi.hoisted(() => ({
  aiGetSettings: vi.fn(),
  aiGetModels: vi.fn(),
  aiSaveSettings: vi.fn(),
  aiAuthStart: vi.fn(),
  aiAuthStatus: vi.fn(),
  aiAuthLogout: vi.fn(),
  aiClearKey: vi.fn(),
}))

vi.mock('@/lib/api', () => ({ api: apiMocks }))

describe('AiTab GitHub OAuth settings', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.aiGetSettings.mockResolvedValue({
      provider: 'copilot',
      enabled: false,
      model: '',
      github_token_set: false,
      github_token_hint: '',
      github_username: '',
      client_id_set: false,
      client_id_builtin: false,
      anthropic_key_set: false,
      anthropic_key_hint: '',
      openai_key_set: false,
      openai_key_hint: '',
      openai_base_url: 'https://api.openai.com/v1',
      ollama_base_url: 'http://localhost:11434',
    })
    apiMocks.aiGetModels.mockRejectedValue(new Error('model catalog unavailable'))
    apiMocks.aiSaveSettings.mockResolvedValue({ success: true })
  })

  it('persists a manual Copilot client ID even when no model could be loaded', async () => {
    render(<AiTab />)

    const input = await screen.findByPlaceholderText('Oauth_…')
    fireEvent.change(input, { target: { value: '  Oauth_test_client  ' } })
    const saveButton = input.parentElement?.querySelector('button')
    expect(saveButton).not.toBeNull()
    fireEvent.click(saveButton!)

    await waitFor(() =>
      expect(apiMocks.aiSaveSettings).toHaveBeenCalledWith({ client_id: 'Oauth_test_client' })
    )
  })

  it('does not allow enabling Copilot before GitHub authentication completes', async () => {
    apiMocks.aiGetSettings.mockResolvedValue({
      provider: 'copilot',
      enabled: false,
      model: 'gpt-4.1',
      github_token_set: false,
      github_token_hint: '',
      github_username: '',
      client_id_set: false,
      client_id_builtin: true,
      anthropic_key_set: false,
      anthropic_key_hint: '',
      openai_key_set: false,
      openai_key_hint: '',
      openai_base_url: 'https://api.openai.com/v1',
      ollama_base_url: 'http://localhost:11434',
    })
    apiMocks.aiGetModels.mockResolvedValue({
      models: [{ id: 'gpt-4.1', name: 'GPT-4.1' }],
    })

    render(<AiTab />)

    fireEvent.click(await screen.findByRole('switch', { name: '启用 AI 助手' }))
    const saveButtons = screen.getAllByRole('button', { name: '保存' })
    expect(saveButtons).not.toHaveLength(0)
    expect(saveButtons.every(button => (button as HTMLButtonElement).disabled)).toBe(true)
    expect(apiMocks.aiSaveSettings).not.toHaveBeenCalled()
  })

  it('treats a stale GitHub username without a token as disconnected', async () => {
    apiMocks.aiGetSettings.mockResolvedValue({
      provider: 'copilot',
      enabled: true,
      model: 'gpt-4.1',
      github_token_set: false,
      github_token_hint: '',
      github_username: 'stale-user',
      client_id_set: false,
      client_id_builtin: true,
      anthropic_key_set: false,
      anthropic_key_hint: '',
      openai_key_set: false,
      openai_key_hint: '',
      openai_base_url: 'https://api.openai.com/v1',
      ollama_base_url: 'http://localhost:11434',
    })

    render(<AiTab />)

    expect(await screen.findByText('未连接')).toBeInTheDocument()
    expect(screen.queryByText(/stale-user/)).not.toBeInTheDocument()
    expect(
      screen
        .getAllByRole('button', { name: '保存' })
        .every(button => button.hasAttribute('disabled'))
    ).toBe(true)
  })
})
