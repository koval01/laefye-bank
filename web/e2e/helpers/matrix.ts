export const VIEWPORTS = [
  { name: 'iphone-se', width: 375, height: 667 },
  { name: 'iphone-12', width: 390, height: 844 },
  { name: 'iphone-14-pro-max', width: 430, height: 932 },
  { name: 'pixel-7', width: 412, height: 915 },
  { name: 'galaxy-s8', width: 360, height: 740 },
  { name: 'ipad-mini', width: 768, height: 1024 },
  { name: 'ipad-pro', width: 1024, height: 1366 },
  { name: 'laptop', width: 1366, height: 768 },
  { name: 'desktop-fhd', width: 1920, height: 1080 },
] as const

export const PUBLIC_ROUTES = [
  { path: '/', title: /любит тебя|laefye/i },
  { path: '/auth', title: /passkey|войти|регистрац/i },
  { path: '/missing-page-xyz', title: /такой страницы нет|404/i },
] as const

export const APP_ROUTES = [
  { path: '/app', marker: /счета|баланс/i },
  { path: '/app/transfer', marker: /перевод/i },
  { path: '/app/crypto', marker: /usdt|trc-20|пополнен/i },
  { path: '/app/fx', marker: /курсы/i },
  { path: '/app/history', marker: /движен/i },
  { path: '/app/account', marker: /аккаунт/i },
  { path: '/app/docs', marker: /документ/i },
  { path: '/app/kyc', marker: /верификац|паспорт|личност/i },
] as const
