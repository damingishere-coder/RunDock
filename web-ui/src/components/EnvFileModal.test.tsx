import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import { EnvFileModal } from './EnvFileModal'

vi.mock('@/lib/api', () => ({
  api: {
    listEnvFiles: vi.fn(),
    getEnvFile: vi.fn(),
    saveEnvFile: vi.fn(),
    restartProcess: vi.fn(),
    syncEnvFiles: vi.fn(),
  },
}))

vi.mock('@/components/EnvEditor', () => ({
  EnvEditor: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => (
    <textarea
      aria-label="环境变量内容"
      value={value}
      onChange={event => onChange(event.target.value)}
    />
  ),
}))

describe('EnvFileModal', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.listEnvFiles).mockResolvedValue({ files: [{ name: '.env', path: '.env' }] })
    vi.mocked(api.getEnvFile).mockResolvedValue({ content: 'A=1', exists: true, filename: '.env' })
    vi.mocked(api.saveEnvFile).mockResolvedValue({ success: true, path: '.env', filename: '.env' })
    vi.mocked(api.restartProcess).mockResolvedValue({} as never)
  })

  it('asks before discarding edits on every close path', async () => {
    const onClose = vi.fn()
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    render(
      <EnvFileModal
        processId="process-1"
        processName="API"
        onClose={onClose}
        onRestarted={vi.fn()}
      />
    )

    const editor = await screen.findByRole('textbox', { name: '环境变量内容' })
    fireEvent.change(editor, { target: { value: 'A=2' } })
    fireEvent.click(screen.getByRole('button', { name: '关闭环境变量编辑器' }))

    expect(confirm).toHaveBeenCalled()
    expect(onClose).not.toHaveBeenCalled()

    confirm.mockReturnValue(true)
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
  })

  it('saves and restarts exactly once before notifying the parent', async () => {
    const onClose = vi.fn()
    const onRestarted = vi.fn()
    render(
      <EnvFileModal
        processId="process-1"
        processName="API"
        onClose={onClose}
        onRestarted={onRestarted}
      />
    )

    const editor = await screen.findByRole('textbox', { name: '环境变量内容' })
    fireEvent.change(editor, { target: { value: 'A=2' } })
    fireEvent.click(screen.getByRole('button', { name: '保存并重启' }))

    await waitFor(() => expect(api.restartProcess).toHaveBeenCalledTimes(1))
    expect(api.saveEnvFile).toHaveBeenCalledWith('process-1', 'A=2', '.env')
    expect(onRestarted).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('keeps the modal open and does not notify the parent when restart fails', async () => {
    vi.mocked(api.restartProcess).mockRejectedValue(new Error('restart unavailable'))
    const onClose = vi.fn()
    const onRestarted = vi.fn()
    render(
      <EnvFileModal
        processId="process-1"
        processName="API"
        onClose={onClose}
        onRestarted={onRestarted}
      />
    )

    const editor = await screen.findByRole('textbox', { name: '环境变量内容' })
    fireEvent.change(editor, { target: { value: 'A=2' } })
    fireEvent.click(screen.getByRole('button', { name: '保存并重启' }))

    expect(await screen.findByText(/\.env 已保存，但进程重启失败/)).toBeInTheDocument()
    expect(api.restartProcess).toHaveBeenCalledTimes(1)
    expect(onRestarted).not.toHaveBeenCalled()
    expect(onClose).not.toHaveBeenCalled()
  })
})
