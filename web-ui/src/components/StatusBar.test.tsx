import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { StatusBar } from './StatusBar'

describe('StatusBar product branding', () => {
  it('shows the RunDock version without exposing the compatibility CLI name', () => {
    render(
      <StatusBar
        connected
        projects={[]}
        statsOpen={false}
        onToggleStats={vi.fn()}
        updateInfo={null}
        onGoToUpdate={vi.fn()}
        version="1.2.3"
        unreadCount={0}
        trayOpen={false}
        onToggleTray={vi.fn()}
        aiOpen={false}
        onToggleAi={vi.fn()}
        devtoolsEnabled={false}
        devtoolsOpen={false}
        onToggleDevtools={vi.fn()}
        terminalState="hidden"
        terminalTabCount={0}
        onToggleTerminal={vi.fn()}
      />
    )

    const version = screen.getByRole('button', { name: '当前版本 1.2.3' })
    expect(version).toHaveAttribute('title', 'RunDock v1.2.3')
    expect(version.getAttribute('title')).not.toContain('alter')
  })
})
