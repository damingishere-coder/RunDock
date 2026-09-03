import { describe, expect, it } from 'vitest'
import { mergeTerminalHistory, terminalHistoryKey } from './terminalHistory'

describe('terminalHistoryKey', () => {
  it('separates same-named processes in different working directories', () => {
    expect(terminalHistoryKey('api', 'C:\\one')).not.toBe(terminalHistoryKey('api', 'C:\\two'))
  })

  it('returns a bounded key and skips generic empty terminals', () => {
    expect(terminalHistoryKey(undefined, '')).toBeUndefined()
    expect(terminalHistoryKey('服务'.repeat(200), '目录'.repeat(200))).toMatch(
      /^proc:[0-9a-f]{16}$/
    )
  })
})

describe('mergeTerminalHistory', () => {
  it('keeps commands from both split panes without double-counting shared history', () => {
    expect(
      mergeTerminalHistory(
        [
          { cmd: 'npm test', count: 3 },
          { cmd: 'cargo test', count: 1 },
        ],
        [
          { cmd: 'npm test', count: 2 },
          { cmd: 'git status', count: 1 },
        ]
      )
    ).toEqual([
      { cmd: 'npm test', count: 3 },
      { cmd: 'cargo test', count: 1 },
      { cmd: 'git status', count: 1 },
    ])
  })
})
