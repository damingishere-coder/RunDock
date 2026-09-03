import { parseArgs, parseDotEnv } from '@/lib/utils'
import type { NotificationConfig, UpdateProcessBody } from '@/types'

export interface ProcessEditDraft {
  script: string
  name: string
  cwd: string
  namespace: string
  args: string
  env: string
  autorestart: boolean
  watch: boolean
  maxRestarts: number
  cron: string
  notify?: NotificationConfig
}

export function buildProcessUpdateBody(draft: ProcessEditDraft): UpdateProcessBody {
  return {
    script: draft.script.trim(),
    ...(draft.name.trim() && { name: draft.name.trim() }),
    cwd: draft.cwd.trim() || null,
    namespace: draft.namespace.trim() || 'default',
    args: draft.args.trim() ? parseArgs(draft.args.trim()) : [],
    env: parseDotEnv(draft.env),
    autorestart: draft.autorestart,
    watch: draft.watch,
    max_restarts: draft.maxRestarts,
    cron: draft.cron.trim() || null,
    notify: draft.notify ?? null,
  }
}
