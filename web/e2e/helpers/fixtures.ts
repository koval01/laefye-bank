import type { Page, Route } from '@playwright/test'

export const ACCOUNT_ID = '11111111-1111-1111-1111-111111111111'
export const ENTRY_ID = 'a8ee7958-eb32-4b9a-92d6-15bfd5a8ee79'

export const meFixture = {
  user_id: '22222222-2222-2222-2222-222222222222',
  username: 'koval01',
  currency: 'RUB',
  profile: {
    user_id: '22222222-2222-2222-2222-222222222222',
    handle: 'koval01',
    display_name: 'Ярослав',
    email: 'yaroslav@example.com',
    phone: '+79001234567',
    full_name: 'Коваль Ярослав',
    birth_date: '1995-01-15',
  },
  accounts: [
    {
      id: ACCOUNT_ID,
      kind: 'checking',
      currency: 'RUB',
      status: 'open',
      balance_minor: 100_000,
    },
  ],
}

export const txnsFixture = [
  {
    entry_id: ENTRY_ID,
    created_at: '2026-08-09T13:36:52.000Z',
    memo: 'Приветственный бонус',
    amount_minor: 100_000,
    currency: 'RUB',
    counterparty_account_id: null,
  },
  {
    entry_id: 'b1b1b1b1-b1b1-b1b1-b1b1-b1b1b1b1b1b1',
    created_at: '2026-08-09T12:00:00.000Z',
    memo: 'Перевод',
    amount_minor: -5_000,
    currency: 'RUB',
    counterparty_account_id: '33333333-3333-3333-3333-333333333333',
  },
]

export const txnDetailFixture = {
  entry_id: ENTRY_ID,
  created_at: '2026-08-09T13:36:52.000Z',
  memo: 'Приветственный бонус',
  status_label: 'Проведена',
  direction_label: 'Зачисление',
  amount_minor: 100_000,
  currency: 'RUB',
  account_id: ACCOUNT_ID,
  idempotency_key: 'welcome:test',
  counterparty: null,
}

export const accountFixture = {
  registered_at: '2026-08-01T10:00:00.000Z',
  profile: meFixture.profile,
  profile_complete: true,
  kyc: { status: 'draft', status_label: 'Черновик', note: '' },
  trust: {
    level: 'basic',
    label: 'Базовый',
    description: 'Стандартные лимиты до верификации.',
  },
  limits: {
    daily_transfer_minor: 50_000_00,
    single_transfer_minor: 15_000_00,
    daily_withdraw_minor: 0,
    crypto_enabled: false,
    note: 'Лимиты для непроверенного аккаунта',
  },
  session: {
    created_at: '2026-08-09T12:00:00.000Z',
    last_seen_at: '2026-08-09T14:00:00.000Z',
    expires_at: '2026-08-10T12:00:00.000Z',
    client_ip: '203.0.113.10',
    client_country: 'DE',
  },
}

export const sessionsFixture = {
  sessions: [
    {
      id: 'sess-current',
      current: true,
      created_at: '2026-08-09T12:00:00.000Z',
      last_seen_at: '2026-08-09T14:00:00.000Z',
      expires_at: '2026-08-10T12:00:00.000Z',
      client_ip: '203.0.113.10',
      client_country: 'DE',
    },
    {
      id: 'sess-other',
      current: false,
      created_at: '2026-08-08T09:00:00.000Z',
      last_seen_at: '2026-08-08T18:00:00.000Z',
      expires_at: '2026-08-09T09:00:00.000Z',
      client_ip: '198.51.100.20',
      client_country: 'NL',
    },
  ],
}

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  })
}

/** Mock customer API for authenticated SPA flows (plain JSON, VITE_E2E_PLAIN_API=1). */
export async function installApiMocks(page: Page) {
  await page.route('**/api/**', async (route) => {
    const req = route.request()
    const url = new URL(req.url())
    const path = url.pathname
    const method = req.method()

    if (path === '/api/session/heartbeat') {
      return json(route, {
        ok: true,
        server_time: new Date().toISOString(),
        session_expires_at: new Date(Date.now() + 3600_000).toISOString(),
        unread: 0,
        idle_secs: 300,
      })
    }
    if (path === '/api/notifications') {
      return json(route, { notifications: [], unread: 0 })
    }
    if (path.startsWith('/api/notifications/')) return json(route, { ok: true })

    if (path === '/api/bank/me' && method === 'GET') return json(route, meFixture)
    if (path === '/api/logout' && method === 'POST') return json(route, { ok: true })
    if (/^\/api\/accounts\/[^/]+\/transactions/.test(path)) {
      return json(route, txnsFixture)
    }
    if (path.startsWith('/api/transactions/')) return json(route, txnDetailFixture)
    if (path === '/api/account' && method === 'GET') return json(route, accountFixture)
    if (path === '/api/account/sessions') return json(route, sessionsFixture)
    if (path.startsWith('/api/account/events')) {
      return json(route, {
        events: [
          {
            id: 'ev1',
            event_label: 'Вход',
            detail: 'Вход · @koval01 · DE',
            created_at: '2026-08-09T12:00:00.000Z',
          },
        ],
      })
    }
    if (path === '/api/account/data-requests') return json(route, { requests: [] })
    if (path === '/api/account/profile' && method === 'POST') {
      return json(route, { profile: meFixture.profile, complete: true })
    }
    if (path === '/api/market') {
      return json(route, {
        usdt_rub: '81.5',
        usdt_usd: '1',
        updated_at: '2026-08-09T14:00:00.000Z',
        cbr: { usd_rub: '92.1', key_rate: '18', inflation: '9.1' },
      })
    }
    if (path === '/api/crypto/settings') {
      return json(route, {
        chain: 'tron',
        asset: 'USDT',
        network: 'TRC-20',
        usdt_rub: 81.5,
        min_deposit_usdt_minor: 1_000_000,
        min_withdraw_usdt_minor: 1_000_000,
        enabled: true,
      })
    }
    if (path === '/api/crypto/deposit-address') {
      return json(route, { address: 'TXYZmockDepositAddressForTests123456' })
    }
    if (path === '/api/crypto/deposits') return json(route, [])
    if (path.startsWith('/api/crypto/quote/')) {
      return json(route, {
        rub_credit_minor: 810_000,
        usdt_out_minor: 10_000_000,
        fee_minor: 0,
      })
    }
    if (path === '/api/kyc' && method === 'GET') {
      return json(route, {
        case: { status: 'draft', note: '' },
        documents: [],
        uploads_enabled: true,
      })
    }
    if (path.startsWith('/api/directory')) {
      return json(route, { handle: 'friend01', display_name: 'Друг' })
    }
    if (path.startsWith('/api/documents/')) {
      return route.fulfill({
        status: 200,
        contentType: 'application/pdf',
        body: '%PDF-1.4 mock',
      })
    }

    return json(route, { ok: true })
  })
}

export async function assertNoHorizontalOverflow(page: Page) {
  const metrics = await page.evaluate(() => {
    const doc = document.documentElement
    const body = document.body
    return {
      clientWidth: doc.clientWidth,
      scrollWidth: Math.max(doc.scrollWidth, body.scrollWidth),
      innerWidth: window.innerWidth,
    }
  })
  if (metrics.scrollWidth > metrics.clientWidth + 1) {
    throw new Error(
      `horizontal overflow: scrollWidth=${metrics.scrollWidth} clientWidth=${metrics.clientWidth}`,
    )
  }
}

export async function assertDialogFitsViewport(page: Page, testId: string) {
  const dialog = page.getByTestId(testId)
  await dialog.waitFor({ state: 'visible' })
  const box = await dialog.boundingBox()
  if (!box) throw new Error('dialog has no box')
  const vp = page.viewportSize()
  if (!vp) throw new Error('no viewport')
  if (box.x < -1 || box.y < -1) {
    throw new Error(`dialog clipped left/top: x=${box.x} y=${box.y}`)
  }
  if (box.x + box.width > vp.width + 1) {
    throw new Error(
      `dialog wider than viewport: right=${box.x + box.width} vp=${vp.width}`,
    )
  }
}
