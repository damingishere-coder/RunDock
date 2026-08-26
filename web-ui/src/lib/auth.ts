// @group Authentication : Session token management

import { getActiveServer, serverTokenKey } from '@/lib/servers'
import type { DaemonTarget } from '@/lib/transport'

type SessionTarget = Pick<DaemonTarget, 'tokenKey'>
const SCREEN_LOCK_KEY = 'alter_screen_locked'

function activeSessionTarget(): SessionTarget {
  return { tokenKey: serverTokenKey(getActiveServer()) }
}

// @group Authentication > Session : Read/write/clear the session token for the active server
export function getSessionToken(target: SessionTarget = activeSessionTarget()): string | null {
  return localStorage.getItem(target.tokenKey)
}

export function setSessionToken(
  token: string,
  target: SessionTarget = activeSessionTarget()
): void {
  localStorage.setItem(target.tokenKey, token)
}

export function clearSessionToken(target: SessionTarget = activeSessionTarget()): void {
  localStorage.removeItem(target.tokenKey)
}

export function isAuthenticated(target?: SessionTarget): boolean {
  return !!getSessionToken(target)
}

export function isScreenLocked(): boolean {
  return localStorage.getItem(SCREEN_LOCK_KEY) === 'true'
}

export function setScreenLocked(locked: boolean): void {
  if (locked) localStorage.setItem(SCREEN_LOCK_KEY, 'true')
  else localStorage.removeItem(SCREEN_LOCK_KEY)
}
