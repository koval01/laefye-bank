import { test, expect } from '@playwright/test'
import { installApiMocks, sessionsFixture, assertNoHorizontalOverflow } from './helpers/fixtures'
import { VIEWPORTS } from './helpers/matrix'

test.beforeEach(async ({ page }) => {
  await installApiMocks(page)
})

test.describe('dashboard', () => {
  test('shows balance and recent movements', async ({ page }) => {
    await page.goto('/app')
    await expect(page.getByText(/1[\s\u00a0]?000/)).toBeVisible()
    await expect(page.getByText(/недавние движения/i)).toBeVisible()
    await expect(page.getByText(/приветственный бонус/i)).toBeVisible()
  })

  test('shows profile completeness when missing fields', async ({ page }) => {
    await page.unroute('**/api/**')
    await installApiMocks(page)
    await page.route('**/api/bank/me', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user_id: '1',
          username: 'x',
          currency: 'RUB',
          profile: {
            user_id: '1',
            handle: 'x',
            display_name: 'X',
            email: '',
            phone: '',
            full_name: '',
            birth_date: null,
          },
          accounts: [
            {
              id: '11111111-1111-1111-1111-111111111111',
              kind: 'checking',
              currency: 'RUB',
              status: 'open',
              balance_minor: 0,
            },
          ],
        }),
      })
    })
    await page.goto('/app')
    await expect(page.getByText(/заполните профиль/i)).toBeVisible()
  })

  test('nav links present', async ({ page }) => {
    await page.goto('/app')
    const nav = page.getByRole('navigation')
    for (const label of ['Главная', 'Перевод', 'USDT', 'Движения', 'Аккаунт']) {
      await expect(nav.getByRole('link', { name: label })).toBeVisible()
    }
  })
})

test.describe('account sessions', () => {
  test('shows RFC3339 dates not Invalid Date', async ({ page }) => {
    await page.goto('/app/account')
    await page.getByRole('button', { name: /сессии и входы/i }).click()
    await expect(page.getByText(/это устройство/i)).toBeVisible()
    await expect(page.getByText(/invalid date/i)).toHaveCount(0)
    await expect(page.getByText(/203\.0\.113\.10/)).toBeVisible()
    await expect(page.getByText(/\bDE\b/).first()).toBeVisible()
  })

  test('lists other session with country', async ({ page }) => {
    await page.goto('/app/account')
    await page.getByRole('button', { name: /сессии и входы/i }).click()
    await expect(page.getByText(/другое устройство/i)).toBeVisible()
    await expect(page.getByText(/198\.51\.100\.20/)).toBeVisible()
    expect(sessionsFixture.sessions).toHaveLength(2)
  })
})

test.describe('history', () => {
  test('opens and closes detail modal', async ({ page }) => {
    await page.setViewportSize({ width: 430, height: 932 })
    await page.goto('/app/history')
    await page.getByRole('button', { name: /приветственный бонус/i }).click()
    await expect(page.getByTestId('txn-detail-modal')).toBeVisible()
    await expect(page.getByText(/зачисление/i)).toBeVisible()
    await page.getByRole('button', { name: /закрыть/i }).click()
    await expect(page.getByTestId('txn-detail-modal')).toHaveCount(0)
  })

  test('escape closes modal', async ({ page }) => {
    await page.goto('/app/history')
    await page.getByRole('button', { name: /приветственный бонус/i }).click()
    await expect(page.getByTestId('txn-detail-modal')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('txn-detail-modal')).toHaveCount(0)
  })
})

for (const vp of VIEWPORTS) {
  test(`bottom nav does not overflow @ ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height })
    await page.goto('/app')
    await expect(page.getByRole('link', { name: 'Главная' })).toBeVisible()
    await assertNoHorizontalOverflow(page)
  })
}
