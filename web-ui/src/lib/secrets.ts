export const MASKED_SECRET = '__RUNDOCK_SECRET_SET__'

export function secretInputValue(value?: string | null): string {
  return value === MASKED_SECRET ? '' : (value ?? '')
}

export function secretInputPlaceholder(value: string | null | undefined, fallback: string): string {
  return value === MASKED_SECRET ? '已设置；留空保持不变' : fallback
}
