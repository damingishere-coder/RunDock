import { useState } from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import { NotificationTray } from './NotificationTray'

function Harness() {
  const [open, setOpen] = useState(false)
  return (
    <MemoryRouter>
      <button onClick={() => setOpen(true)}>打开活动</button>
      <NotificationTray
        open={open}
        notifications={[]}
        onClose={() => setOpen(false)}
        onMarkAllRead={vi.fn()}
        onClearAll={vi.fn()}
        onDismiss={vi.fn()}
      />
    </MemoryRouter>
  )
}

describe('NotificationTray', () => {
  it('moves focus into the dialog, traps Tab, and restores focus after Escape', async () => {
    render(<Harness />)
    const opener = screen.getByRole('button', { name: '打开活动' })
    opener.focus()
    fireEvent.click(opener)

    const close = screen.getByRole('button', { name: '关闭通知活动' })
    await waitFor(() => expect(close).toHaveFocus())
    fireEvent.keyDown(window, { key: 'Tab' })
    expect(screen.getByRole('button', { name: '打开通知设置' })).toHaveFocus()

    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(opener).toHaveFocus())
  })
})
