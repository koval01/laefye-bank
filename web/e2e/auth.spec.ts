import { test, expect } from '@playwright/test'
import { VIEWPORTS } from './helpers/matrix'
import { assertNoHorizontalOverflow } from './helpers/fixtures'

test.describe('auth page', () => {
  test('shows register and login modes', async ({ page }) => {
    await page.goto('/auth')
    await expect(page.getByRole('button', { name: /регистрация/i })).toBeVisible()
    await expect(page.getByRole('button', { name: /войти/i })).toBeVisible()
  })

  test('register mode shows display name field', async ({ page }) => {
    await page.goto('/auth')
    await page.getByRole('button', { name: /регистрация/i }).click()
    await expect(page.getByLabel(/отображаемое имя/i)).toBeVisible()
    await expect(page.getByRole('button', { name: /создать passkey/i })).toBeVisible()
  })

  test('login mode hides display name field', async ({ page }) => {
    await page.goto('/auth')
    await page.getByRole('button', { name: /войти/i }).click()
    await expect(page.getByLabel(/отображаемое имя/i)).toHaveCount(0)
    await expect(page.getByRole('button', { name: /войти с passkey/i })).toBeVisible()
  })

  test('username input accepts text', async ({ page }) => {
    await page.goto('/auth')
    await page.getByLabel(/имя пользователя/i).fill('demo_user')
    await expect(page.getByLabel(/имя пользователя/i)).toHaveValue('demo_user')
  })

  for (const vp of VIEWPORTS) {
    test(`auth form usable @ ${vp.name}`, async ({ page }) => {
      await page.setViewportSize({ width: vp.width, height: vp.height })
      await page.goto('/auth')
      const submit = page.getByRole('button', { name: /passkey/i }).first()
      await expect(submit).toBeVisible()
      const box = await submit.boundingBox()
      expect(box).toBeTruthy()
      expect(box!.width).toBeGreaterThan(80)
      await assertNoHorizontalOverflow(page)
    })
  }
})
