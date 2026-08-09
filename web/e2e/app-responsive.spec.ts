import { test, expect } from '@playwright/test'
import {
  assertDialogFitsViewport,
  assertNoHorizontalOverflow,
  installApiMocks,
} from './helpers/fixtures'
import { APP_ROUTES, VIEWPORTS } from './helpers/matrix'

test.beforeEach(async ({ page }) => {
  await installApiMocks(page)
})

for (const vp of VIEWPORTS) {
  for (const route of APP_ROUTES) {
    test(`app ${route.path} @ ${vp.name}: no horizontal overflow`, async ({ page }) => {
      await page.setViewportSize({ width: vp.width, height: vp.height })
      await page.goto(route.path)
      await expect(page.getByText(route.marker).first()).toBeVisible({ timeout: 15_000 })
      await assertNoHorizontalOverflow(page)
    })
  }
}

for (const vp of VIEWPORTS.filter((v) => v.width <= 430)) {
  test(`history detail modal fits @ ${vp.name}`, async ({ page }) => {
    await page.setViewportSize({ width: vp.width, height: vp.height })
    await page.goto('/app/history')
    await expect(page.getByText(/движен/i).first()).toBeVisible()
    await page.getByRole('button', { name: /приветственный бонус/i }).click()
    await expect(page.getByTestId('txn-detail-modal')).toBeVisible()
    await assertDialogFitsViewport(page, 'txn-detail-modal')
    await assertNoHorizontalOverflow(page)
  })
}
