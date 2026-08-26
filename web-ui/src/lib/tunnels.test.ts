import { describe, expect, it } from 'vitest'
import { parseInstallStreamEvent } from './tunnels'

describe('parseInstallStreamEvent', () => {
  it('accepts only typed line and completion events', () => {
    expect(parseInstallStreamEvent('{"line":"downloading"}')).toEqual({ line: 'downloading' })
    expect(parseInstallStreamEvent('{"done":true,"ok":false}')).toEqual({
      done: true,
      ok: false,
    })
  })

  it('rejects malformed and ambiguous payloads', () => {
    expect(parseInstallStreamEvent('{')).toBeNull()
    expect(parseInstallStreamEvent('{"done":false,"ok":true}')).toBeNull()
    expect(parseInstallStreamEvent('{"line":42}')).toBeNull()
  })
})
