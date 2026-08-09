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
