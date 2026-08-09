import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatMoney(minor: number, currency = 'RUB'): string {
  return new Intl.NumberFormat('ru-RU', {
    style: 'currency',
    currency,
    minimumFractionDigits: 2,
  }).format(minor / 100)
}

/** @deprecated use formatMoney */
export function formatEur(minor: number): string {
  return formatMoney(minor, 'RUB')
}

export function newIdempotencyKey(): string {
  if (crypto.randomUUID) return crypto.randomUUID()
  return `ik_${Date.now()}_${Math.random().toString(36).slice(2)}`
}

export function greeting(): string {
  const h = new Date().getHours()
  if (h < 6) return 'Ты ещё не спишь?'
  if (h < 12) return 'Доброе утро'
  if (h < 18) return 'Привет'
  return 'Добрый вечер'
}

/** Map legacy / raw API messages to Russian copy for the customer UI. */
export function userFacingError(err: unknown): string {
  const code =
    err && typeof err === 'object' && 'code' in err
      ? String((err as { code?: string }).code || '')
      : ''
  const raw = err instanceof Error ? err.message : String(err)
  const byCode: Record<string, string> = {
    origin_mismatch:
      'Сессия банка устарела после обновления. Обновите страницу и попробуйте снова.',
    authentication_failed: 'Не удалось подтвердить passkey. Проверьте имя и попробуйте снова.',
    invalid_ceremony: 'Не удалось проверить passkey. Попробуйте другое устройство или браузер.',
    rate_limited: 'Слишком много попыток. Подождите немного и повторите.',
    conflict: 'Такой пользователь уже зарегистрирован — войдите.',
    secure_channel_required: 'Обновите страницу — требуется защищённый канал.',
    pow_failed: 'Не удалось подтвердить клиент. Обновите страницу.',
    channel_expired: 'Защищённый канал истёк. Обновите страницу.',
    session_network_changed: 'Сеть сменилась — войдите снова.',
    session_ua_changed: 'Клиент изменился — войдите снова.',
    session_fingerprint_changed: 'Устройство не совпало — войдите снова.',
    session_expired: 'Сессия истекла — войдите снова.',
    session_proof_mismatch: 'Сессия повреждена — войдите снова.',
    step_up_required: 'Подтвердите действие passkey.',
    no_session: 'Войдите в аккаунт',
  }
  if (code && byCode[code]) return byCode[code]

  const map: Record<string, string> = {
    'not found': 'Пользователь с таким @ником не найден',
    'recipient not found': 'Получатель не найден',
    'account not found': 'Счёт не найден',
    'no session': 'Войдите в аккаунт',
    'session expired': 'Сессия истекла — войдите снова',
    'authentication failed': byCode.authentication_failed,
    'origin mismatch': byCode.origin_mismatch,
    'Passkey creation cancelled': 'Создание passkey отменено',
    'Passkey login cancelled': 'Вход с passkey отменён',
  }
  return map[raw] || map[raw.toLowerCase()] || raw
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return '—'
  const d = new Date(value)
  if (Number.isNaN(d.getTime())) return '—'
  return d.toLocaleString('ru-RU')
}

export function formatDate(value: string | null | undefined): string {
  if (!value) return '—'
  const d = new Date(value)
  if (Number.isNaN(d.getTime())) return '—'
  return d.toLocaleDateString('ru-RU')
}

export async function fetchPdf(url: string): Promise<Blob> {
  const { ApiError } = await import('@/lib/api')
  const extra = await import('@/lib/device').then((m) => m.deviceHeaders())
  const res = await fetch(url, {
    credentials: 'same-origin',
    headers: { ...extra },
  })
  if (!res.ok) {
    let msg = `HTTP ${res.status}`
    let code: string | undefined
    try {
      const data = (await res.json()) as { error?: string; code?: string }
      if (data.error) msg = data.error
      code = data.code
    } catch {
      /* ignore */
    }
    throw new ApiError(res.status, msg, code)
  }
  return res.blob()
}
