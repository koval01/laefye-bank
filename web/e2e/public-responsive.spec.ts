import { test, expect } from '@playwright/test'
import { assertNoHorizontalOverflow } from './helpers/fixtures'
import { PUBLIC_ROUTES, VIEWPORTS } from './helpers/matrix'

for (const vp of VIEWPORTS) {
  for (const route of PUBLIC_ROUTES) {
    test(`public ${route.path} @ ${vp.name}: no horizontal overflow`, async ({ page }) => {
      await page.setViewportSize({ width: vp.width, height: vp.height })
      await page.goto(route.path)
      await expect(page.locator('body')).toBeVisible()
      await assertNoHorizontalOverflow(page)
    })

    test(`public ${route.path} @ ${vp.name}: content visible`, async ({ page }) => {
      await page.setViewportSize({ width: vp.width, height: vp.height })
      await page.goto(route.path)
      await expect(page.locator('body')).toContainText(route.title)
    })
  }
}
