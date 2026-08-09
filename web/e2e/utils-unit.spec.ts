import { test, expect } from '@playwright/test'
import { formatDateTime, formatMoney, greeting, userFacingError } from '../src/lib/utils'
import { dedupeTxns, type Txn } from '../src/lib/api'

// Pure helpers — run in Playwright Node process (no browser).

test.describe('formatMoney', () => {
  const cases: [number, string, RegExp][] = [
    [0, 'RUB', /0[,.]00/],
    [100, 'RUB', /1[,.]00/],
    [100_000, 'RUB', /1[\s\u00a0]?000/],
    [-5_050, 'RUB', /-/],
    [1, 'USD', /0[,.]01|1/],
    [99, 'EUR', /0[,.]99|99/],
  ]
  for (const [minor, ccy, re] of cases) {
    test(`formatMoney(${minor}, ${ccy})`, () => {
      expect(formatMoney(minor, ccy)).toMatch(re)
    })
  }
})

test.describe('formatDateTime', () => {
  test('parses ISO', () => {
    expect(formatDateTime('2026-08-09T13:36:52.000Z')).not.toBe('—')
    expect(formatDateTime('2026-08-09T13:36:52.000Z')).not.toMatch(/invalid/i)
  })
  test('empty → dash', () => {
    expect(formatDateTime(null)).toBe('—')
    expect(formatDateTime(undefined)).toBe('—')
    expect(formatDateTime('')).toBe('—')
  })
  test('garbage → dash', () => {
    expect(formatDateTime('not-a-date')).toBe('—')
  })
})

test.describe('greeting', () => {
  test('returns non-empty Russian string', () => {
    expect(greeting().length).toBeGreaterThan(2)
  })
})

test.describe('userFacingError', () => {
  const codes = [
    'origin_mismatch',
    'authentication_failed',
    'invalid_ceremony',
    'rate_limited',
    'conflict',
    'secure_channel_required',
    'pow_failed',
    'channel_expired',
  ]
  for (const code of codes) {
    test(`maps code ${code}`, () => {
      const msg = userFacingError({ code, message: 'x' })
      expect(msg.length).toBeGreaterThan(5)
      expect(msg).not.toBe('x')
    })
  }
  test('maps known raw messages', () => {
    expect(userFacingError(new Error('not found'))).toMatch(/ник/i)
    expect(userFacingError(new Error('recipient not found'))).toMatch(/получатель/i)
  })
})

test.describe('dedupeTxns', () => {
  test('keeps first by entry_id', () => {
    const rows: Txn[] = [
      {
        entry_id: 'a',
        created_at: '1',
        memo: 'one',
        amount_minor: 1,
        currency: 'RUB',
        counterparty_account_id: null,
      },
      {
        entry_id: 'a',
        created_at: '2',
        memo: 'dup',
        amount_minor: 1,
        currency: 'RUB',
        counterparty_account_id: null,
      },
      {
        entry_id: 'b',
        created_at: '3',
        memo: 'two',
        amount_minor: 2,
        currency: 'RUB',
        counterparty_account_id: null,
      },
    ]
    const out = dedupeTxns(rows)
    expect(out).toHaveLength(2)
    expect(out[0].memo).toBe('one')
  })
})
