import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import { DEFAULT_SETTINGS } from '@/lib/settings'
import StartPage from './StartPage'

vi.mock('@/lib/api', () => ({
  api: {
    listEnvPath: vi.fn(),
    checkEnvPath: vi.fn(),
    readEnvFile: vi.fn(),
    writeEnvFile: vi.fn(),
    startProcess: vi.fn(),
    getProcesses: vi.fn(),
  },
}))

describe('StartPage environment file errors', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.listEnvPath).mockResolvedValue({
      files: [{ name: '.env', path: 'C:\\project\\.env' }],
    })
    vi.mocked(api.readEnvFile).mockResolvedValue({ content: 'PORT=3000', exists: true })
    vi.mocked(api.getProcesses).mockResolvedValue({ processes: [] })
  })

  it('shows a failed env save instead of reporting success', async () => {
    const user = userEvent.setup()
    vi.mocked(api.writeEnvFile).mockRejectedValue(new Error('磁盘写入失败'))

    render(
      <MemoryRouter>
        <StartPage onDone={vi.fn()} settings={DEFAULT_SETTINGS} />
      </MemoryRouter>
    )

    fireEvent.change(screen.getByPlaceholderText('C:\\Users\\me\\app'), {
      target: { value: 'C:\\project' },
    })
    await waitFor(() => expect(api.readEnvFile).toHaveBeenCalledWith('C:\\project\\.env'))

    const editor = screen.getByPlaceholderText('KEY=value')
    fireEvent.change(editor, { target: { value: 'PORT=3000\nHOST=127.0.0.1' } })
    await user.click(screen.getByRole('button', { name: '保存' }))

    expect(await screen.findByText('磁盘写入失败')).toBeVisible()
    expect(screen.queryByText('✓ 已保存')).not.toBeInTheDocument()
  })

  it('keeps dirty env content when a cwd change is declined', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false)

    render(
      <MemoryRouter>
        <StartPage onDone={vi.fn()} settings={DEFAULT_SETTINGS} />
      </MemoryRouter>
    )

    const cwdInput = screen.getByPlaceholderText('C:\\Users\\me\\app')
    fireEvent.change(cwdInput, { target: { value: 'C:\\project' } })
    await waitFor(() => expect(api.readEnvFile).toHaveBeenCalled())
    fireEvent.change(screen.getByPlaceholderText('KEY=value'), {
      target: { value: 'PORT=3000\nDIRTY=1' },
    })

    fireEvent.change(cwdInput, { target: { value: 'C:\\other' } })

    expect(window.confirm).toHaveBeenCalled()
    expect(cwdInput).toHaveValue('C:\\project')
    expect(screen.getByPlaceholderText('KEY=value')).toHaveValue('PORT=3000\nDIRTY=1')
  })
})
