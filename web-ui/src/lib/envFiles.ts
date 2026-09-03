// @group Utilities > EnvColor : Shared color coding for environment-file variants

export function envFileColor(name: string): string {
  if (name === '.env') return '#4ade80'
  if (name === '.env.example') return '#fbbf24'
  if (name === '.env.local') return '#60a5fa'
  if (name === '.env.production' || name === '.env.prod') return '#f87171'
  if (name === '.env.development' || name === '.env.dev') return '#34d399'
  if (name === '.env.test') return '#a78bfa'
  if (name === '.env.staging') return '#fb923c'
  return '#94a3b8'
}

export function envFileBg(name: string): string {
  if (name === '.env') return 'rgba(74,222,128,0.13)'
  if (name === '.env.example') return 'rgba(251,191,36,0.13)'
  if (name === '.env.local') return 'rgba(96,165,250,0.13)'
  if (name === '.env.production' || name === '.env.prod') return 'rgba(248,113,113,0.13)'
  if (name === '.env.development' || name === '.env.dev') return 'rgba(52,211,153,0.13)'
  if (name === '.env.test') return 'rgba(167,139,250,0.13)'
  if (name === '.env.staging') return 'rgba(251,146,60,0.13)'
  return 'rgba(148,163,184,0.1)'
}
