import { describe, expect, it } from 'vitest'
import { buildProcessUpdateBody } from './processPatch'

describe('buildProcessUpdateBody', () => {
  it('sends explicit clears for nullable values and empty arrays', () => {
    expect(
      buildProcessUpdateBody({
        script: ' app.exe ',
        name: ' app ',
        cwd: ' ',
        namespace: '',
        args: '',
        env: '',
        autorestart: false,
        watch: false,
        maxRestarts: 3,
        cron: '',
        notify: undefined,
      })
    ).toEqual({
      script: 'app.exe',
      name: 'app',
      cwd: null,
      namespace: 'default',
      args: [],
      env: {},
      autorestart: false,
      watch: false,
      max_restarts: 3,
      cron: null,
      notify: null,
    })
  })
})
