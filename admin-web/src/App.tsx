import { useCallback, useEffect, useState } from 'react'
import { Navigate, Route, Routes, useNavigate } from 'react-router-dom'
import { Fingerprint } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, Input, Label } from '@/components/ui/input'
import { api, ApiError, loginPasskey, registerPasskey } from '@/lib/api'

function AuthPage() {
  const nav = useNavigate()
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [username, setUsername] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      if (mode === 'register') {
        await registerPasskey(username.trim())
      } else {
        await loginPasskey(username.trim() || undefined)
      }
      nav('/')
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-auto flex min-h-dvh w-full max-w-md flex-col justify-center px-6">
      <p className="font-display text-sm font-semibold tracking-wide text-pink">Laefye Ops</p>
      <h1 className="mt-2 font-display text-3xl font-semibold">Вход в консоль</h1>
      <p className="mt-2 text-sm text-fog">Корпоративный доступ. Сессия только для сотрудников.</p>
      <Card className="mt-8">
        <div className="mb-4 flex gap-2">
          <Button
            type="button"
            size="sm"
            variant={mode === 'login' ? 'default' : 'soft'}
            onClick={() => setMode('login')}
          >
            Войти
          </Button>
          <Button
            type="button"
            size="sm"
            variant={mode === 'register' ? 'default' : 'soft'}
            onClick={() => setMode('register')}
          >
            Зарегистрировать ключ
          </Button>
        </div>
        <form className="space-y-4" onSubmit={submit}>
          <div>
            <Label>Имя пользователя</Label>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="username"
              required={mode === 'register'}
            />
          </div>
          {error && <p className="text-sm text-pink">{error}</p>}
          <Button className="w-full" type="submit" disabled={busy}>
            <Fingerprint className="h-4 w-4" />
            {busy ? 'Ожидание…' : mode === 'register' ? 'Создать passkey' : 'Войти'}
          </Button>
        </form>
      </Card>
    </div>
  )
}

function Console() {
  const nav = useNavigate()
  const [handle, setHandle] = useState<string | null>(null)
  const [margin, setMargin] = useState('50')
  const [spread, setSpread] = useState('30')
  const [depSpread, setDepSpread] = useState('80')
  const [wdSpread, setWdSpread] = useState('120')
  const [usdtMid, setUsdtMid] = useState('1')
  const [cases, setCases] = useState<{ id: string; status: string; user_id: string }[]>([])
  const [msg, setMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const me = await api<{ handle: string }>('GET', '/api/admin/me')
      setHandle(me.handle)
      const s = await api<{ margin_bps: number; spread_bps: number }>('GET', '/api/admin/fx/settings')
      setMargin(String(s.margin_bps))
      setSpread(String(s.spread_bps))
      const c = await api<{
        settings: {
          deposit_spread_bps: number
          withdraw_spread_bps: number
          usdt_usd_mid: string
        }
      }>('GET', '/api/admin/crypto/settings')
      setDepSpread(String(c.settings.deposit_spread_bps))
      setWdSpread(String(c.settings.withdraw_spread_bps))
      setUsdtMid(String(c.settings.usdt_usd_mid))
      setCases(await api('GET', '/api/admin/kyc/cases'))
    } catch (err) {
      if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
        nav('/auth')
        return
      }
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [nav])

  useEffect(() => {
    void load()
  }, [load])

  async function logout() {
    try {
      await api('POST', '/api/logout')
    } catch {
      /* ignore */
    }
    nav('/auth')
  }

  async function saveFx(e: React.FormEvent) {
    e.preventDefault()
    try {
      await api('POST', '/api/admin/fx/settings', {
        margin_bps: Number(margin),
        spread_bps: Number(spread),
        enabled_currencies: ['RUB'],
      })
      setMsg('Настройки FX сохранены')
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  async function saveCrypto(e: React.FormEvent) {
    e.preventDefault()
    try {
      await api('POST', '/api/admin/crypto/settings', {
        usdt_usd_mid: usdtMid,
        deposit_spread_bps: Number(depSpread),
        withdraw_spread_bps: Number(wdSpread),
        min_deposit_usdt_minor: 1_000_000,
        min_withdraw_usdt_minor: 5_000_000,
        enabled: true,
      })
      setMsg('Настройки USDT сохранены')
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  async function refreshCbr() {
    try {
      const r = await api<{ rates: number }>('POST', '/api/admin/fx/refresh')
      setMsg(`Обновлено курсов: ${r.rates}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  async function review(id: string, status: string) {
    await api('POST', `/api/admin/kyc/cases/${id}`, { status, note: status })
    await load()
  }

  if (loading) {
    return <p className="p-10 text-fog">Загрузка…</p>
  }

  return (
    <div className="mx-auto min-h-dvh w-full max-w-5xl px-6 py-8">
      <header className="mb-8 flex items-center justify-between gap-4 border-b border-line pb-6">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-pink">Ops</p>
          <h1 className="font-display text-3xl font-semibold">Консоль</h1>
          {handle && <p className="mt-1 text-sm text-fog">@{handle}</p>}
        </div>
        <Button variant="outline" size="sm" onClick={() => void logout()}>
          Выйти
        </Button>
      </header>

      <div className="grid gap-5 lg:grid-cols-2">
        <Card>
          <h2 className="font-semibold">ЦБ РФ</h2>
          <p className="mt-1 text-sm text-fog">
            Воркеры тянут SOAP DailyInfo + XML_daily. Кнопка — принудительное обновление курсов.
          </p>
          <form className="mt-4 space-y-3" onSubmit={saveFx}>
            <div>
              <Label>Маржа (bps, legacy FX)</Label>
              <Input value={margin} onChange={(e) => setMargin(e.target.value)} />
            </div>
            <div>
              <Label>Спред (bps)</Label>
              <Input value={spread} onChange={(e) => setSpread(e.target.value)} />
            </div>
            <div className="flex flex-col gap-2 sm:flex-row">
              <Button type="submit" className="flex-1">
                Сохранить
              </Button>
              <Button type="button" variant="outline" className="flex-1" onClick={() => void refreshCbr()}>
                Обновить ЦБ РФ
              </Button>
            </div>
          </form>
        </Card>

        <Card>
          <h2 className="font-semibold">Рынок USDT / RUB</h2>
          <form className="mt-4 space-y-3" onSubmit={saveCrypto}>
            <div>
              <Label>USDT/USD mid (ручной override)</Label>
              <Input value={usdtMid} onChange={(e) => setUsdtMid(e.target.value)} />
              <p className="mt-1 text-xs text-fog">
                Обычно пишет воркер (CoinGecko/Binance). Ручная правка — аварийный рычаг.
              </p>
            </div>
            <div>
              <Label>Спред пополнения</Label>
              <Input value={depSpread} onChange={(e) => setDepSpread(e.target.value)} />
            </div>
            <div>
              <Label>Спред вывода</Label>
              <Input value={wdSpread} onChange={(e) => setWdSpread(e.target.value)} />
            </div>
            <Button type="submit" className="w-full">
              Сохранить
            </Button>
          </form>
        </Card>
      </div>

      <Card className="mt-5">
        <h2 className="mb-4 font-semibold">Очередь KYC</h2>
        <ul className="space-y-2 text-sm">
          {cases.map((c) => (
            <li
              key={c.id}
              className="flex flex-col gap-2 rounded-xl bg-panel-2 p-3 sm:flex-row sm:items-center sm:justify-between"
            >
              <div>
                <p className="font-medium">{c.status}</p>
                <p className="truncate text-xs text-fog">{c.id}</p>
              </div>
              <div className="flex gap-2">
                <Button size="sm" onClick={() => void review(c.id, 'approved')}>
                  Одобрить
                </Button>
                <Button size="sm" variant="outline" onClick={() => void review(c.id, 'rejected')}>
                  Отклонить
                </Button>
              </div>
            </li>
          ))}
          {!cases.length && <li className="text-fog">Заявок нет</li>}
        </ul>
      </Card>

      {error && <p className="mt-4 text-sm text-pink">{error}</p>}
      {msg && <p className="mt-4 text-sm text-mint">{msg}</p>}
    </div>
  )
}

export default function App() {
  return (
    <Routes>
      <Route path="/auth" element={<AuthPage />} />
      <Route path="/" element={<Console />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
