import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, Navigate, Route, Routes, useNavigate } from 'react-router-dom'
import {
  ArrowUpRight,
  CheckCircle2,
  FileText,
  Fingerprint,
  RefreshCw,
  ShieldCheck,
  Upload,
} from 'lucide-react'
import { Shell } from '@/components/Shell'
import { Logo } from '@/components/Logo'
import { ModalShell } from '@/components/ModalShell'
import { Button } from '@/components/ui/button'
import { Card, Input, Label } from '@/components/ui/input'
import {
  api,
  type BankMe,
  type Txn,
  dedupeTxns,
  loginPasskey,
  registerPasskey,
  requirePasskeyThen,
  withStepUp,
} from '@/lib/api'
import { resetSecureSession } from '@/secure/channel'
import {
  formatMoney,
  formatDateTime,
  greeting,
  newIdempotencyKey,
  userFacingError,
  fetchPdf,
} from '@/lib/utils'
import { getVisitorId } from '@/lib/device'
import { useSessionWatch } from '@/lib/session-watch'

function NotFoundPage() {
  return (
    <div className="mx-auto flex min-h-dvh w-full max-w-lg flex-col items-center justify-center px-6 py-10">
      <Logo to="/" size="md" animated className="mb-8" />
      <Card className="w-full space-y-3 text-center animate-rise">
        <p
          className="font-display text-5xl font-bold tracking-tight love-text sm:text-6xl"
          aria-hidden
        >
          404
        </p>
        <h1 className="font-display text-xl font-semibold tracking-tight sm:text-2xl">
          Такой страницы нет
        </h1>
        <p className="text-sm leading-relaxed text-fog">
          Ссылка устарела или адрес набран с ошибкой. Деньги и счета на месте — просто вернитесь
          в банк.
        </p>
        <div className="flex flex-wrap justify-center gap-2 pt-3">
          <Link to="/">
            <Button>На главную</Button>
          </Link>
          <Link to="/auth">
            <Button variant="outline">Войти</Button>
          </Link>
        </div>
      </Card>
    </div>
  )
}

function Landing() {
  return (
    <div className="relative min-h-dvh overflow-hidden">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_80%_60%_at_70%_-10%,rgba(255,61,127,0.18),transparent_55%)]" />

      <div className="relative mx-auto flex min-h-dvh w-full max-w-6xl flex-col px-6 py-8 sm:px-10 lg:px-12">
        <header className="flex items-center justify-between">
          <Logo size="lg" animated />
          <nav className="flex items-center gap-2 sm:gap-3">
            <Link to="/auth?mode=login">
              <Button variant="ghost" size="sm">
                Войти
              </Button>
            </Link>
            <Link to="/auth">
              <Button size="sm">Открыть счёт</Button>
            </Link>
          </nav>
        </header>

        <section className="flex flex-1 flex-col justify-center gap-10 py-16 lg:grid lg:grid-cols-[1.15fr_0.85fr] lg:items-center lg:gap-16 lg:py-20">
          <div>
            <h1 className="font-display text-4xl font-semibold leading-[1.08] tracking-tight sm:text-5xl lg:text-6xl">
              Банк, который
              <br />
              <span className="love-text">любит тебя</span>
            </h1>
            <p className="mt-6 max-w-xl text-base leading-relaxed text-fog sm:text-lg">
              Счёт за минуту, вход по passkey, переводы по @нику и пополнение в USDT. Без паролей и
              без отделений.
            </p>
            <div className="mt-8 flex flex-col gap-3 sm:flex-row sm:items-center">
              <Link to="/auth" className="sm:w-auto">
                <Button size="lg" className="w-full sm:w-auto">
                  Открыть счёт
                </Button>
              </Link>
              <Link to="/auth?mode=login" className="sm:w-auto">
                <Button size="lg" variant="outline" className="w-full sm:w-auto">
                  Войти
                </Button>
              </Link>
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-3 lg:grid-cols-1">
            {[
              ['Passkey', 'Вход по Face ID / Touch ID — пароль не нужен'],
              ['@переводы', 'Отправляй деньги по имени, как в мессенджере'],
              ['USDT', 'Пополнение и вывод через TRC-20'],
            ].map(([title, body]) => (
              <div key={title} className="rounded-2xl border border-line/80 bg-panel/60 px-5 py-4">
                <p className="font-display text-sm font-semibold text-pink">{title}</p>
                <p className="mt-1 text-sm leading-relaxed text-fog">{body}</p>
              </div>
            ))}
          </div>
        </section>

        <footer className="border-t border-line/60 py-5 text-xs text-fog">
          © {new Date().getFullYear()} Laefye
        </footer>
      </div>
    </div>
  )
}

function AuthPage() {
  const nav = useNavigate()
  const [mode, setMode] = useState<'register' | 'login'>(() =>
    new URLSearchParams(window.location.search).get('mode') === 'login' ? 'login' : 'register',
  )
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    void getVisitorId().catch(() => undefined)
  }, [])

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setBusy(true)
    try {
      if (mode === 'register') {
        await registerPasskey(username.trim(), displayName.trim() || undefined)
      } else {
        await loginPasskey(username.trim() || undefined)
      }
      nav('/app')
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-auto flex min-h-dvh w-full max-w-lg flex-col justify-center px-6 py-10">
      <Logo to="/" size="md" className="mb-8" />
      <Card>
        <h2 className="font-display text-2xl font-semibold">
          {mode === 'register' ? 'Открытие счёта' : 'Вход'}
        </h2>
        <p className="mt-2 text-sm text-fog">
          {mode === 'register'
            ? 'Создайте passkey на устройстве — пароль не потребуется.'
            : 'Подтвердите вход passkey на этом устройстве.'}
        </p>
        <div className="mt-5 mb-5 flex gap-2">
          <Button
            type="button"
            size="sm"
            variant={mode === 'register' ? 'default' : 'soft'}
            onClick={() => setMode('register')}
          >
            Регистрация
          </Button>
          <Button
            type="button"
            size="sm"
            variant={mode === 'login' ? 'default' : 'soft'}
            onClick={() => setMode('login')}
          >
            Войти
          </Button>
        </div>
        <form className="space-y-4" onSubmit={submit}>
          <div>
            <Label htmlFor="user">Имя пользователя</Label>
            <Input
              id="user"
              autoComplete="username webauthn"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="username"
              required={mode === 'register'}
            />
          </div>
          {mode === 'register' && (
            <div>
              <Label htmlFor="display">Отображаемое имя</Label>
              <Input
                id="display"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="Имя"
              />
            </div>
          )}
          {error && <p className="text-sm text-pink">{error}</p>}
          <Button className="w-full" type="submit" disabled={busy}>
            <Fingerprint className="h-4 w-4" />
            {busy
              ? 'Ожидание ключа…'
              : mode === 'register'
                ? 'Создать passkey'
                : 'Войти с passkey'}
          </Button>
        </form>
      </Card>
    </div>
  )
}

function useBankMe() {
  const [me, setMe] = useState<BankMe | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await api<BankMe>('GET', '/api/bank/me')
      setMe(data)
    } catch (err) {
      setMe(null)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return { me, error, loading, refresh }
}

function Onboarding({ onDone }: { onDone: () => void }) {
  const [handle, setHandle] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await withStepUp(() =>
        api('POST', '/api/bank/profile', {
          handle,
          display_name: displayName || undefined,
        }),
      )
      onDone()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card>
      <h2 className="font-display text-2xl font-semibold">Выберите @ник</h2>
      <p className="mt-2 text-sm text-fog">
        По нему вам будут переводить деньги. При открытии рублёвого счёта зачисляется приветственный
        бонус ₽1 000.
      </p>
      <form className="mt-5 space-y-4" onSubmit={submit}>
        <div>
          <Label htmlFor="handle">Ник</Label>
          <Input
            id="handle"
            value={handle}
            onChange={(e) => setHandle(e.target.value.toLowerCase())}
            placeholder="username"
            required
          />
        </div>
        <div>
          <Label htmlFor="dn">Отображаемое имя</Label>
          <Input id="dn" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
        </div>
        {error && <p className="text-sm text-pink">{error}</p>}
        <Button className="w-full" type="submit" disabled={busy}>
          Сохранить и открыть счёт
        </Button>
      </form>
    </Card>
  )
}

function Dashboard({ me, refresh }: { me: BankMe; refresh: () => void }) {
  const accounts = me.accounts.filter((a) => a.kind === 'checking')
  const primary = accounts.find((a) => a.currency === 'RUB') ?? accounts[0]
  const [txns, setTxns] = useState<Txn[]>([])
  const accountId = primary?.id
  const profileIncomplete =
    !me.profile?.email ||
    !me.profile?.phone ||
    !me.profile?.full_name ||
    !me.profile?.birth_date

  useEffect(() => {
    if (!accountId) return
    void (async () => {
      try {
        const rows = await api<Txn[]>('GET', `/api/accounts/${accountId}/transactions?limit=8`)
        setTxns(dedupeTxns(rows))
      } catch {
        /* dashboard stays usable without history */
      }
    })()
  }, [accountId])

  return (
    <div className="space-y-5">
      <div>
        <p className="text-sm text-fog">
          {greeting()}, {me.profile?.display_name || me.username}
        </p>
        <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">Счета</h1>
      </div>

      <div className="rounded-2xl border border-line bg-panel p-5 sm:p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-fog">Баланс</p>
        <p className="mt-2 font-display text-4xl font-semibold tracking-tight sm:text-5xl">
          {formatMoney(primary?.balance_minor ?? 0, primary?.currency ?? 'RUB')}
        </p>
        <p className="mt-2 text-sm text-fog">@{me.profile?.handle}</p>
        <div className="mt-6 flex gap-2">
          <Link to="/app/transfer" className="flex-1">
            <Button className="w-full">
              <ArrowUpRight className="h-4 w-4" />
              Перевести
            </Button>
          </Link>
          <Link to="/app/crypto" className="flex-1">
            <Button variant="outline" className="w-full">
              USDT
            </Button>
          </Link>
          <Button variant="outline" onClick={refresh} aria-label="Обновить">
            <RefreshCw className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {accounts.length > 1 && (
        <div className="grid gap-2 sm:grid-cols-2">
          {accounts.map((a) => (
            <Card key={a.id} className="flex items-center justify-between py-4">
              <span className="text-sm text-fog">{a.currency}</span>
              <span className="font-semibold">{formatMoney(a.balance_minor, a.currency)}</span>
            </Card>
          ))}
        </div>
      )}

      {profileIncomplete && (
        <Card>
          <div className="flex items-start justify-between gap-3">
            <div>
              <h3 className="font-semibold">Заполните профиль</h3>
              <p className="mt-1 text-sm leading-relaxed text-fog">
                Для базового использования нужны почта, телефон, ФИО и дата рождения.
              </p>
            </div>
            <Link to="/app/account">
              <Button variant="outline" size="sm">
                Заполнить
              </Button>
            </Link>
          </div>
        </Card>
      )}

      <Card>
        <div className="flex items-start justify-between gap-3">
          <div>
            <h3 className="font-semibold">Верификация</h3>
            <p className="mt-1 text-sm leading-relaxed text-fog">
              Подтвердите личность, чтобы снять лимиты на переводы и вывод.
            </p>
          </div>
          <Link to="/app/kyc">
            <Button variant="outline" size="sm">
              <ShieldCheck className="h-4 w-4" />
              Пройти
            </Button>
          </Link>
        </div>
      </Card>

      <Card>
        <div className="mb-2 flex items-center justify-between gap-3">
          <h3 className="font-semibold">Недавние движения</h3>
          <Link to="/app/history" className="text-sm text-fog hover:text-snow">
            Все
          </Link>
        </div>
        <ul className="divide-y divide-line/70">
          {txns.map((t) => (
            <li key={t.entry_id} className="flex items-center justify-between py-3 text-sm">
              <div className="min-w-0">
                <p className="truncate font-medium">{t.memo || 'Операция'}</p>
                <p className="text-xs text-fog">{formatDateTime(t.created_at)}</p>
              </div>
              <p className={`shrink-0 font-semibold ${t.amount_minor >= 0 ? 'text-mint' : ''}`}>
                {formatMoney(t.amount_minor, t.currency)}
              </p>
            </li>
          ))}
          {!txns.length && (
            <li className="py-4 text-center text-sm text-fog">Операций пока нет</li>
          )}
        </ul>
      </Card>
    </div>
  )
}

function TransferPage({ me, onDone }: { me: BankMe; onDone: () => void }) {
  const [handle, setHandle] = useState('')
  const [amount, setAmount] = useState('1000')
  const [memo, setMemo] = useState('')
  const [lookup, setLookup] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [ok, setOk] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const bal = me.accounts.find((a) => a.kind === 'checking' && a.currency === 'RUB')
    ?.balance_minor ?? 0

  async function doLookup() {
    setLookup(null)
    setError(null)
    try {
      const r = await api<{ display_name: string; handle: string }>(
        'GET',
        `/api/directory/lookup?handle=${encodeURIComponent(handle)}`,
      )
      setLookup(`${r.display_name} · @${r.handle}`)
    } catch (err) {
      setError(userFacingError(err))
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    setOk(null)
    try {
      const rubles = Number(amount)
      if (!Number.isFinite(rubles) || rubles <= 0) throw new Error('Сумма должна быть больше нуля')
      const amount_minor = Math.round(rubles * 100)
      const res = await requirePasskeyThen(() =>
        api<{ from_balance_minor: number }>('POST', '/api/transfers', {
          to_handle: handle,
          amount_minor,
          currency: 'RUB',
          idempotency_key: newIdempotencyKey(),
          memo,
        }),
      )
      setOk(`Готово. Остаток ${formatMoney(res.from_balance_minor)}`)
      onDone()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card>
      <h2 className="font-display text-2xl font-semibold">Перевод</h2>
      <p className="mt-1 text-sm text-fog">
        Доступно {formatMoney(bal)} · без комиссии между клиентами банка.
        Перевод подтверждается passkey.
      </p>
      <form className="mt-5 space-y-4" onSubmit={submit}>
        <div>
          <Label htmlFor="to">Кому (@ник)</Label>
          <div className="flex gap-2">
            <Input
              id="to"
              value={handle}
              onChange={(e) => {
                setHandle(e.target.value.toLowerCase().replace(/^@/, ''))
                setLookup(null)
              }}
              placeholder="username"
              required
            />
            <Button type="button" variant="outline" onClick={doLookup}>
              Найти
            </Button>
          </div>
          {lookup && <p className="mt-2 text-sm text-mint">{lookup}</p>}
        </div>
        <div>
          <Label htmlFor="amt">Сумма (₽)</Label>
          <Input id="amt" value={amount} onChange={(e) => setAmount(e.target.value)} required />
        </div>
        <div>
          <Label htmlFor="memo">Назначение</Label>
          <Input
            id="memo"
            value={memo}
            onChange={(e) => setMemo(e.target.value)}
            placeholder="кофе / долг / просто так"
          />
        </div>
        {error && (
          <p className="rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
            {error}
          </p>
        )}
        {ok && <p className="text-sm text-mint">{ok}</p>}
        <Button className="w-full" type="submit" disabled={busy}>
          {busy ? 'Отправка…' : 'Отправить'}
        </Button>
      </form>
    </Card>
  )
}

function RatesPage() {
  const [market, setMarket] = useState<{
    usdt_rub: string
    usdt_usd: string
    updated_at: string | null
    cbr: {
      usd_rub: string | null
      key_rate: string | null
      inflation: string | null
    }
  } | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      try {
        setMarket(await api('GET', '/api/market'))
      } catch (err) {
        setError(userFacingError(err))
      }
    })()
  }, [])

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="font-display text-2xl font-semibold">Курсы</h2>
        <p className="mt-1 text-sm text-fog">
          В банке работают рубли и USDT. Курс USDT/RUB обновляется автоматически, официальный
          USD/RUB — по данным ЦБ РФ.
        </p>
      </div>
      {error && (
        <p className="rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
          {error}
        </p>
      )}
      <Card className="space-y-3">
        <h3 className="font-medium">USDT ↔ ₽</h3>
        {market ? (
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between gap-3">
              <dt className="text-fog">1 USDT</dt>
              <dd className="font-semibold">
                {Number(market.usdt_rub).toLocaleString('ru-RU', {
                  maximumFractionDigits: 4,
                })}{' '}
                ₽
              </dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-fog">USDT ≈ USD</dt>
              <dd>{Number(market.usdt_usd).toFixed(6)}</dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-fog">Обновлено</dt>
              <dd className="text-fog">
                {market.updated_at
                  ? new Date(market.updated_at).toLocaleString('ru-RU')
                  : 'ожидаем обновление'}
              </dd>
            </div>
          </dl>
        ) : (
          <p className="text-sm text-fog">Загрузка…</p>
        )}
        <Link to="/app/crypto">
          <Button variant="outline" className="w-full">
            Пополнить / вывести USDT
          </Button>
        </Link>
      </Card>
      <Card className="space-y-3">
        <h3 className="font-medium">ЦБ РФ</h3>
        {market ? (
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between gap-3">
              <dt className="text-fog">USD / RUB</dt>
              <dd>
                {market.cbr.usd_rub
                  ? Number(market.cbr.usd_rub).toLocaleString('ru-RU', {
                      maximumFractionDigits: 4,
                    })
                  : '—'}
              </dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-fog">Ключевая ставка</dt>
              <dd>{market.cbr.key_rate ? `${market.cbr.key_rate}%` : '—'}</dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt className="text-fog">Инфляция</dt>
              <dd>{market.cbr.inflation ? `${market.cbr.inflation}%` : '—'}</dd>
            </div>
          </dl>
        ) : (
          <p className="text-sm text-fog">Загрузка…</p>
        )}
      </Card>
    </div>
  )
}

function PdfPreview({
  title,
  url,
  onClose,
}: {
  title: string
  url: string
  onClose: () => void
}) {
  const src = url.includes('#') ? url : `${url}#toolbar=0&navpanes=0&view=FitH`
  return (
    <ModalShell onBackdrop={onClose} className="max-w-3xl">
      <div className="flex h-[min(88dvh,860px)] w-full flex-col overflow-hidden rounded-2xl border border-line bg-panel shadow-2xl">
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-line px-3 py-3 sm:px-4">
          <h3 className="truncate font-display text-base font-semibold sm:text-lg">{title}</h3>
          <div className="flex shrink-0 gap-2">
            <a href={url} download className="inline-flex">
              <Button type="button" variant="outline" size="sm">
                Скачать
              </Button>
            </a>
            <Button type="button" variant="ghost" size="sm" onClick={onClose}>
              Закрыть
            </Button>
          </div>
        </div>
        <div className="relative min-h-0 w-full flex-1 bg-panel-2">
          <iframe title={title} src={src} className="absolute inset-0 h-full w-full border-0 bg-white" />
        </div>
      </div>
    </ModalShell>
  )
}

type TxnDetail = {
  entry_id: string
  created_at: string
  memo: string
  status_label: string
  direction_label: string
  amount_minor: number
  currency: string
  account_id: string
  idempotency_key: string
  counterparty: { handle: string; display_name: string } | null
}

function HistoryPage({ me }: { me: BankMe }) {
  const [txns, setTxns] = useState<Txn[]>([])
  const [error, setError] = useState<string | null>(null)
  const [detail, setDetail] = useState<TxnDetail | null>(null)
  const [detailBusy, setDetailBusy] = useState(false)
  const [pdf, setPdf] = useState<{ title: string; url: string } | null>(null)
  const accountId =
    me.accounts.find((a) => a.kind === 'checking' && a.currency === 'RUB')?.id ??
    me.accounts.find((a) => a.kind === 'checking')?.id

  useEffect(() => {
    if (!accountId) return
    void (async () => {
      try {
        setTxns(dedupeTxns(await api<Txn[]>('GET', `/api/accounts/${accountId}/transactions?limit=50`)))
      } catch (err) {
        setError(userFacingError(err))
      }
    })()
  }, [accountId])

  useEffect(() => {
    return () => {
      if (pdf?.url) URL.revokeObjectURL(pdf.url)
    }
  }, [pdf])

  async function openDetail(entryId: string) {
    setDetailBusy(true)
    setError(null)
    try {
      setDetail(await api<TxnDetail>('GET', `/api/transactions/${entryId}`))
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setDetailBusy(false)
    }
  }

  async function openReceipt(entryId: string) {
    setError(null)
    try {
      const blob = await fetchPdf(`/api/documents/receipt/${entryId}`)
      if (pdf?.url) URL.revokeObjectURL(pdf.url)
      setPdf({ title: 'Квитанция', url: URL.createObjectURL(blob) })
    } catch (err) {
      setError(userFacingError(err))
    }
  }

  return (
    <div className="animate-rise space-y-4">
      <div className="flex items-end justify-between gap-3">
        <div>
          <h2 className="font-display text-2xl font-semibold">Движения</h2>
          <p className="mt-1 text-sm text-fog">Нажмите на операцию, чтобы открыть детали</p>
        </div>
        <Link to="/app/docs">
          <Button variant="outline" size="sm">
            Документы
          </Button>
        </Link>
      </div>

      <Card>
        {error && (
          <p className="mb-3 rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
            {error}
          </p>
        )}
        <ul className="divide-y divide-line/70">
          {txns.map((t) => (
            <li key={t.entry_id}>
              <button
                type="button"
                className="flex w-full items-center justify-between py-3.5 text-left transition hover:bg-panel-2/60"
                onClick={() => void openDetail(t.entry_id)}
              >
                <div>
                  <p className="text-sm font-medium">{t.memo || 'Операция'}</p>
                  <p className="text-xs text-fog">{formatDateTime(t.created_at)}</p>
                </div>
                <p className={`font-semibold ${t.amount_minor >= 0 ? 'text-mint' : 'text-snow'}`}>
                  {formatMoney(t.amount_minor, t.currency)}
                </p>
              </button>
            </li>
          ))}
          {!txns.length && !error && (
            <li className="py-8 text-center text-sm text-fog">Операций пока нет</li>
          )}
        </ul>
      </Card>

      {(detail || detailBusy) && (
        <ModalShell zClass="z-40" onBackdrop={() => setDetail(null)}>
          <Card className="w-full space-y-3" data-testid="txn-detail-modal">
            {detailBusy && !detail ? (
              <p className="text-sm text-fog">Загрузка…</p>
            ) : detail ? (
              <>
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="text-xs uppercase tracking-wide text-fog">
                      {detail.direction_label}
                    </p>
                    <h3 className="font-display text-xl font-semibold">
                      {formatMoney(detail.amount_minor, detail.currency)}
                    </h3>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="shrink-0"
                    onClick={() => setDetail(null)}
                  >
                    Закрыть
                  </Button>
                </div>
                <dl className="space-y-2 text-sm">
                  <div className="flex justify-between gap-3">
                    <dt className="shrink-0 text-fog">Статус</dt>
                    <dd className="text-right">{detail.status_label}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="shrink-0 text-fog">Дата</dt>
                    <dd className="text-right">{formatDateTime(detail.created_at)}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="shrink-0 text-fog">Назначение</dt>
                    <dd className="min-w-0 break-words text-right">{detail.memo || '—'}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="shrink-0 text-fog">Контрагент</dt>
                    <dd className="min-w-0 break-words text-right">
                      {detail.counterparty
                        ? `${detail.counterparty.display_name} · @${detail.counterparty.handle}`
                        : '—'}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="shrink-0 text-fog">ID операции</dt>
                    <dd className="max-w-[55%] truncate font-mono text-xs">{detail.entry_id}</dd>
                  </div>
                </dl>
                <Button
                  className="w-full"
                  type="button"
                  onClick={() => void openReceipt(detail.entry_id)}
                >
                  Квитанция PDF
                </Button>
              </>
            ) : null}
          </Card>
        </ModalShell>
      )}

      {pdf && (
        <PdfPreview
          title={pdf.title}
          url={pdf.url}
          onClose={() => {
            URL.revokeObjectURL(pdf.url)
            setPdf(null)
          }}
        />
      )}
    </div>
  )
}

function AccountPage({ me, onDone }: { me: BankMe; onDone: () => void }) {
  const [tab, setTab] = useState<'overview' | 'security' | 'data'>('overview')
  const [account, setAccount] = useState<{
    registered_at: string | null
    profile: {
      handle: string
      display_name: string
      email?: string
      phone?: string
      full_name?: string
      birth_date?: string | null
    } | null
    profile_complete?: boolean
    kyc: { status: string; status_label: string; note: string }
    trust: { level: string; label: string; description: string }
    limits: {
      daily_transfer_minor: number
      single_transfer_minor: number
      daily_withdraw_minor: number
      crypto_enabled: boolean
      note: string
    }
    session: {
      created_at: string
      last_seen_at: string
      expires_at: string
      client_ip?: string | null
      client_country?: string | null
    }
  } | null>(null)
  const [sessions, setSessions] = useState<
    {
      id: string
      current: boolean
      created_at: string
      last_seen_at: string
      expires_at: string
      client_ip?: string | null
      client_country?: string | null
    }[]
  >([])
  const [events, setEvents] = useState<
    { id: string; event_label: string; detail: string; created_at: string }[]
  >([])
  const [requests, setRequests] = useState<
    { id: string; request_type: string; payload: string; status: string; created_at: string }[]
  >([])
  const [displayName, setDisplayName] = useState(me.profile?.display_name || '')
  const [email, setEmail] = useState(me.profile?.email || '')
  const [phone, setPhone] = useState(me.profile?.phone || '')
  const [fullName, setFullName] = useState(me.profile?.full_name || '')
  const [birthDate, setBirthDate] = useState(me.profile?.birth_date || '')
  const [reqType, setReqType] = useState('personal_data')
  const [reqPayload, setReqPayload] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    const a = await api<NonNullable<typeof account>>('GET', '/api/account')
    setAccount(a)
    setDisplayName(a.profile?.display_name || me.profile?.display_name || '')
    setEmail(a.profile?.email || '')
    setPhone(a.profile?.phone || '')
    setFullName(a.profile?.full_name || '')
    setBirthDate(a.profile?.birth_date || '')
    const s = await api<{ sessions: typeof sessions }>('GET', '/api/account/sessions')
    setSessions(s.sessions)
    const e = await api<{ events: typeof events }>('GET', '/api/account/events?limit=40')
    setEvents(e.events)
    const r = await api<{ requests: typeof requests }>('GET', '/api/account/data-requests')
    setRequests(r.requests)
  }, [me.profile?.display_name])

  useEffect(() => {
    void load().catch((err) => setError(userFacingError(err)))
  }, [load])

  async function saveProfile(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    setMsg(null)
    try {
      await requirePasskeyThen(() =>
        api('POST', '/api/account/profile', {
          display_name: displayName,
          email,
          phone,
          full_name: fullName,
          birth_date: birthDate || '',
        }),
      )
      setMsg('Профиль сохранён')
      onDone()
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  async function revoke(id: string) {
    setBusy(true)
    setError(null)
    try {
      await requirePasskeyThen(() =>
        api('POST', `/api/account/sessions/${encodeURIComponent(id)}`),
      )
      setMsg('Сессия завершена')
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  async function revokeOthers() {
    setBusy(true)
    setError(null)
    try {
      const r = await requirePasskeyThen(() =>
        api<{ revoked: number }>('POST', '/api/account/sessions/revoke-others'),
      )
      setMsg(`Завершено сессий: ${r.revoked}`)
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  async function submitDataRequest(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    setMsg(null)
    try {
      await api('POST', '/api/account/data-requests', {
        request_type: reqType,
        payload: reqPayload,
      })
      setMsg('Заявка отправлена')
      setReqPayload('')
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  const reqTypeLabel: Record<string, string> = {
    personal_data: 'Личные данные',
    address: 'Адрес',
    contacts: 'Контакты',
    other: 'Другое',
  }

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="font-display text-2xl font-semibold">Аккаунт</h2>
        <p className="mt-1 text-sm text-fog">
          Профиль, доверие банка, лимиты, сессии и заявки на обновление данных.
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        {(
          [
            ['overview', 'Обзор'],
            ['security', 'Сессии и входы'],
            ['data', 'Данные'],
          ] as const
        ).map(([id, label]) => (
          <Button
            key={id}
            size="sm"
            variant={tab === id ? 'default' : 'soft'}
            onClick={() => setTab(id)}
          >
            {label}
          </Button>
        ))}
      </div>

      {error && (
        <p className="rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
          {error}
        </p>
      )}
      {msg && <p className="text-sm text-mint">{msg}</p>}

      {tab === 'overview' && account && (
        <div className="space-y-4">
          <Card className="space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="rounded-full bg-mint/15 px-3 py-1 text-xs font-semibold text-mint">
                {account.trust.label}
              </span>
              <span className="rounded-full bg-panel-2 px-3 py-1 text-xs text-fog">
                KYC: {account.kyc.status_label}
              </span>
            </div>
            <p className="text-sm leading-relaxed text-fog">{account.trust.description}</p>
            <dl className="grid gap-2 text-sm sm:grid-cols-2">
              <div>
                <dt className="text-fog">@ник</dt>
                <dd className="font-medium">@{account.profile?.handle || me.profile?.handle}</dd>
              </div>
              <div>
                <dt className="text-fog">Логин</dt>
                <dd className="font-medium">{me.username}</dd>
              </div>
              <div>
                <dt className="text-fog">Регистрация</dt>
                <dd className="font-medium">{formatDateTime(account.registered_at)}</dd>
              </div>
              <div>
                <dt className="text-fog">Текущая сессия до</dt>
                <dd className="font-medium">{formatDateTime(account.session.expires_at)}</dd>
              </div>
              <div>
                <dt className="text-fog">Ваш IP</dt>
                <dd className="font-medium">
                  {account.session.client_ip || '—'}
                  {account.session.client_country ? ` · ${account.session.client_country}` : ''}
                </dd>
              </div>
              <div>
                <dt className="text-fog">ФИО</dt>
                <dd className="font-medium">{account.profile?.full_name || '—'}</dd>
              </div>
              <div>
                <dt className="text-fog">Email</dt>
                <dd className="font-medium">{account.profile?.email || '—'}</dd>
              </div>
              <div>
                <dt className="text-fog">Телефон</dt>
                <dd className="font-medium">{account.profile?.phone || '—'}</dd>
              </div>
            </dl>
            <div className="flex flex-wrap gap-2 pt-1">
              <Link to="/app/kyc">
                <Button variant="outline" size="sm">
                  Верификация
                </Button>
              </Link>
              <Link to="/app/docs">
                <Button variant="outline" size="sm">
                  Документы PDF
                </Button>
              </Link>
            </div>
          </Card>

          <Card className="space-y-3">
            <h3 className="font-medium">Лимиты</h3>
            <p className="text-sm text-fog">{account.limits.note}</p>
            <ul className="space-y-2 text-sm">
              <li className="flex justify-between gap-3">
                <span className="text-fog">Перевод за раз</span>
                <span>{formatMoney(account.limits.single_transfer_minor)}</span>
              </li>
              <li className="flex justify-between gap-3">
                <span className="text-fog">Переводы в сутки</span>
                <span>{formatMoney(account.limits.daily_transfer_minor)}</span>
              </li>
              <li className="flex justify-between gap-3">
                <span className="text-fog">Вывод в сутки</span>
                <span>
                  {account.limits.daily_withdraw_minor > 0
                    ? formatMoney(account.limits.daily_withdraw_minor)
                    : 'закрыт'}
                </span>
              </li>
              <li className="flex justify-between gap-3">
                <span className="text-fog">USDT</span>
                <span>{account.limits.crypto_enabled ? 'доступен' : 'недоступен'}</span>
              </li>
            </ul>
          </Card>

          <Card>
            <h3 className="font-medium">Профиль</h3>
            <p className="mt-1 text-sm text-fog">
              Почта, телефон, ФИО и дата рождения нужны для базового использования. @ник не
              меняется.
            </p>
            <form className="mt-4 space-y-3" onSubmit={saveProfile}>
              <div>
                <Label>Отображаемое имя</Label>
                <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
              </div>
              <div>
                <Label>ФИО</Label>
                <Input
                  value={fullName}
                  onChange={(e) => setFullName(e.target.value)}
                  placeholder="Иванов Иван Иванович"
                />
              </div>
              <div>
                <Label>Email</Label>
                <Input
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="name@example.com"
                  autoComplete="email"
                />
              </div>
              <div>
                <Label>Телефон</Label>
                <Input
                  type="tel"
                  value={phone}
                  onChange={(e) => setPhone(e.target.value)}
                  placeholder="+79001234567"
                  autoComplete="tel"
                />
              </div>
              <div>
                <Label>Дата рождения</Label>
                <Input
                  type="date"
                  value={birthDate}
                  onChange={(e) => setBirthDate(e.target.value)}
                />
              </div>
              <Button type="submit" disabled={busy}>
                Сохранить
              </Button>
            </form>
          </Card>
        </div>
      )}

      {tab === 'security' && (
        <div className="space-y-4">
          <Card className="space-y-3">
            <div className="flex items-center justify-between gap-3">
              <h3 className="font-medium">Активные сессии</h3>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => void revokeOthers()}
              >
                Завершить другие
              </Button>
            </div>
            <ul className="space-y-2">
              {sessions.map((s) => (
                <li
                  key={s.id}
                  className="flex items-center justify-between gap-3 rounded-2xl bg-panel-2 px-4 py-3 text-sm"
                >
                  <div className="min-w-0">
                    <p className="font-medium">
                      {s.current ? 'Это устройство' : 'Другое устройство'}
                    </p>
                    <p className="text-xs text-fog">
                      вход {formatDateTime(s.created_at)} · активность{' '}
                      {formatDateTime(s.last_seen_at)}
                    </p>
                    {(s.client_ip || s.client_country) && (
                      <p className="text-xs text-fog">
                        {s.client_ip || 'IP неизвестен'}
                        {s.client_country ? ` · ${s.client_country}` : ''}
                      </p>
                    )}
                  </div>
                  {!s.current && (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={busy}
                      onClick={() => void revoke(s.id)}
                    >
                      Завершить
                    </Button>
                  )}
                </li>
              ))}
              {!sessions.length && <li className="text-sm text-fog">Сессий нет</li>}
            </ul>
          </Card>

          <Card className="space-y-3">
            <h3 className="font-medium">Журнал входов</h3>
            <ul className="divide-y divide-line/70">
              {events.map((ev) => (
                <li key={ev.id} className="flex items-start justify-between gap-3 py-3 text-sm">
                  <div>
                    <p className="font-medium">{ev.event_label}</p>
                    <p className="text-xs text-fog">{ev.detail || '—'}</p>
                  </div>
                  <span className="shrink-0 text-xs text-fog">
                    {formatDateTime(ev.created_at)}
                  </span>
                </li>
              ))}
              {!events.length && (
                <li className="py-4 text-center text-sm text-fog">Пока пусто — войдите ещё раз</li>
              )}
            </ul>
          </Card>
        </div>
      )}

      {tab === 'data' && (
        <div className="space-y-4">
          <Card>
            <h3 className="font-medium">Обновление данных</h3>
            <p className="mt-1 text-sm text-fog">
              Сообщите банку об изменении ФИО, адреса или контактов. Для документов личности
              используйте верификацию.
            </p>
            <form className="mt-4 space-y-3" onSubmit={submitDataRequest}>
              <div>
                <Label>Тип</Label>
                <select
                  value={reqType}
                  onChange={(e) => setReqType(e.target.value)}
                  className="flex h-12 w-full rounded-2xl border border-line bg-panel px-4 text-sm text-snow focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pink/40"
                >
                  {Object.entries(reqTypeLabel).map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <Label>Что изменить</Label>
                <textarea
                  value={reqPayload}
                  onChange={(e) => setReqPayload(e.target.value)}
                  rows={4}
                  placeholder="Новые данные и комментарий"
                  className="w-full rounded-2xl border border-line bg-panel px-4 py-3 text-sm text-snow placeholder:text-fog/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pink/40"
                  required
                />
              </div>
              <Button type="submit" disabled={busy}>
                Отправить заявку
              </Button>
            </form>
          </Card>

          {requests.length > 0 && (
            <Card className="space-y-2">
              <h3 className="font-medium">Мои заявки</h3>
              <ul className="space-y-2 text-sm">
                {requests.map((r) => (
                  <li key={r.id} className="rounded-2xl bg-panel-2 px-4 py-3">
                    <div className="flex justify-between gap-3">
                      <span className="font-medium">{reqTypeLabel[r.request_type] || r.request_type}</span>
                      <span className="text-xs text-fog">{r.status}</span>
                    </div>
                    <p className="mt-1 text-fog">{r.payload}</p>
                    <p className="mt-1 text-xs text-fog">
                      {formatDateTime(r.created_at)}
                    </p>
                  </li>
                ))}
              </ul>
            </Card>
          )}

          <Card>
            <h3 className="font-medium">Документы личности</h3>
            <p className="mt-1 text-sm text-fog">Паспорт и селфи — в разделе верификации.</p>
            <Link to="/app/kyc" className="mt-4 inline-flex">
              <Button variant="outline">Открыть KYC</Button>
            </Link>
          </Card>
        </div>
      )}
    </div>
  )
}

function DocsPage({ me }: { me: BankMe }) {
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [pdf, setPdf] = useState<{ title: string; url: string } | null>(null)
  const accountId = me.accounts.find((a) => a.kind === 'checking' && a.currency === 'RUB')?.id

  useEffect(() => {
    return () => {
      if (pdf?.url) URL.revokeObjectURL(pdf.url)
    }
  }, [pdf])

  async function openStatement(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const qs = new URLSearchParams()
      if (accountId) qs.set('account_id', accountId)
      if (from) qs.set('from', new Date(from).toISOString())
      if (to) {
        const end = new Date(to)
        end.setHours(23, 59, 59, 999)
        qs.set('to', end.toISOString())
      }
      const blob = await withStepUp(() =>
        fetchPdf(`/api/documents/statement?${qs.toString()}`),
      )
      if (pdf?.url) URL.revokeObjectURL(pdf.url)
      setPdf({ title: 'Выписка по счёту', url: URL.createObjectURL(blob) })
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="font-display text-2xl font-semibold">Документы</h2>
        <p className="mt-1 text-sm text-fog">
          Выписки и квитанции — фирменный PDF Laefye Bank с DSM-печатью
          (TSE-подобный QR и цепочка подписей).
        </p>
      </div>

      <Card>
        <h3 className="font-medium">Выписка по рублёвому счёту</h3>
        <p className="mt-1 text-sm text-fog">
          Укажите период или оставьте пустым — будут последние операции.
          Период длиннее месяца или без даты «С» потребует повторный passkey.
        </p>
        <form className="mt-4 space-y-3" onSubmit={openStatement}>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <Label>С</Label>
              <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
            </div>
            <div>
              <Label>По</Label>
              <Input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
            </div>
          </div>
          {error && (
            <p className="rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
              {error}
            </p>
          )}
          <Button className="w-full" type="submit" disabled={busy || !accountId}>
            {busy ? 'Формируем…' : 'Сформировать и открыть'}
          </Button>
        </form>
      </Card>

      <Card>
        <h3 className="font-medium">Квитанция по операции</h3>
        <p className="mt-1 text-sm text-fog">
          Откройте операцию во вкладке «Движения» и нажмите «Квитанция PDF».
        </p>
        <Link to="/app/history" className="mt-4 inline-flex">
          <Button variant="outline">К движениям</Button>
        </Link>
      </Card>

      <Card>
        <h3 className="font-medium">Курсы RUB / USDT</h3>
        <p className="mt-1 text-sm text-fog">Рыночный USDT и индикаторы ЦБ РФ.</p>
        <Link to="/app/fx" className="mt-4 inline-flex">
          <Button variant="outline">Открыть курсы</Button>
        </Link>
      </Card>

      {pdf && (
        <PdfPreview
          title={pdf.title}
          url={pdf.url}
          onClose={() => {
            URL.revokeObjectURL(pdf.url)
            setPdf(null)
          }}
        />
      )}
    </div>
  )
}

function formatUsdt(minor: number) {
  return (minor / 1_000_000).toFixed(2)
}

function CryptoPage({ me, onDone }: { me: BankMe; onDone: () => void }) {
  const [tab, setTab] = useState<'in' | 'out'>('in')
  const [address, setAddress] = useState<string | null>(null)
  const [settings, setSettings] = useState<{
    min_deposit_usdt_minor: number
    min_withdraw_usdt_minor: number
  } | null>(null)
  const [usdtAmount, setUsdtAmount] = useState('10')
  const [depositQuote, setDepositQuote] = useState<{
    rub_credit_minor: number
    fee_minor: number
  } | null>(null)
  const [rubAmount, setRubAmount] = useState('1000')
  const [toAddress, setToAddress] = useState('')
  const [withdrawQuote, setWithdrawQuote] = useState<{
    usdt_out_minor: number
    fee_minor: number
  } | null>(null)
  const [deposits, setDeposits] = useState<
    { tx_hash: string; usdt_minor: number; rub_credit_minor: number; fee_minor: number; status: string }[]
  >([])
  const [msg, setMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const rubBal =
    me.accounts.find((a) => a.kind === 'checking' && a.currency === 'RUB')?.balance_minor ?? 0

  const load = useCallback(async () => {
    const s = await api<{
      min_deposit_usdt_minor: number
      min_withdraw_usdt_minor: number
    }>('GET', '/api/crypto/settings')
    setSettings(s)
    try {
      const a = await api<{ address: string }>('GET', '/api/crypto/deposit-address')
      setAddress(a.address)
    } catch {
      setAddress(null)
    }
    setDeposits(await api('GET', '/api/crypto/deposits'))
  }, [])

  useEffect(() => {
    void load().catch((e) => setError(String(e)))
  }, [load])

  useEffect(() => {
    const usdt = Number(usdtAmount)
    if (!Number.isFinite(usdt) || usdt <= 0) {
      setDepositQuote(null)
      return
    }
    const usdt_minor = Math.round(usdt * 1_000_000)
    void api<{ rub_credit_minor: number; fee_minor: number }>(
      'GET',
      `/api/crypto/quote/deposit?usdt_minor=${usdt_minor}`,
    )
      .then(setDepositQuote)
      .catch(() => setDepositQuote(null))
  }, [usdtAmount])

  useEffect(() => {
    const rub = Number(rubAmount)
    if (!Number.isFinite(rub) || rub <= 0) {
      setWithdrawQuote(null)
      return
    }
    const rub_minor = Math.round(rub * 100)
    void api<{ usdt_out_minor: number; fee_minor: number }>(
      'GET',
      `/api/crypto/quote/withdraw?rub_minor=${rub_minor}`,
    )
      .then(setWithdrawQuote)
      .catch(() => setWithdrawQuote(null))
  }, [rubAmount])

  async function copyAddr() {
    if (!address) return
    await navigator.clipboard.writeText(address)
    setMsg('Адрес скопирован')
  }

  async function withdraw(e: React.FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError(null)
    setMsg(null)
    try {
      await api('POST', '/api/accounts/open', { currency: 'RUB' })
      const rub = Number(rubAmount)
      const rub_minor = Math.round(rub * 100)
      const res = await requirePasskeyThen(() =>
        api<{ usdt_out_minor: number; fee_minor: number; status: string }>(
          'POST',
          '/api/crypto/withdraw',
          {
            rub_minor,
            to_address: toAddress.trim(),
            idempotency_key: newIdempotencyKey(),
          },
        ),
      )
      setMsg(
        `Вывод в очереди → ${formatUsdt(res.usdt_out_minor)} USDT (комиссия ${formatMoney(res.fee_minor)}). Статус: ${res.status}`,
      )
      onDone()
      await load()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="font-display text-2xl font-semibold">USDT</h2>
        <p className="mt-1 text-sm text-fog">
          Пополнение и вывод через сеть TRON (TRC-20). Зачисление на рублёвый счёт.
        </p>
      </div>

      <div className="flex gap-2">
        <Button size="sm" variant={tab === 'in' ? 'default' : 'soft'} onClick={() => setTab('in')}>
          Пополнить
        </Button>
        <Button size="sm" variant={tab === 'out' ? 'default' : 'soft'} onClick={() => setTab('out')}>
          Вывести
        </Button>
      </div>

      {tab === 'in' ? (
        <Card className="space-y-4">
          <p className="text-sm text-fog">
            Отправьте <span className="font-semibold text-snow">USDT TRC-20</span> на адрес ниже.
            Зачисление автоматическое.
          </p>
          {address ? (
            <div className="rounded-2xl bg-panel-2 p-3">
              <p className="break-all font-mono text-xs leading-relaxed">{address}</p>
              <Button type="button" variant="outline" className="mt-3 w-full" onClick={() => void copyAddr()}>
                Скопировать адрес
              </Button>
            </div>
          ) : (
            <p className="text-sm text-pink">Адрес депозита временно недоступен.</p>
          )}
          <div>
            <Label>Сумма USDT</Label>
            <Input value={usdtAmount} onChange={(e) => setUsdtAmount(e.target.value)} />
            {depositQuote && (
              <p className="mt-2 text-sm text-mint">
                На счёт придёт {formatMoney(depositQuote.rub_credit_minor)}
                {depositQuote.fee_minor > 0
                  ? ` · комиссия ${formatMoney(depositQuote.fee_minor)}`
                  : ''}
              </p>
            )}
            {settings && (
              <p className="mt-1 text-xs text-fog">
                Минимум {formatUsdt(settings.min_deposit_usdt_minor)} USDT
              </p>
            )}
          </div>
          {deposits.length > 0 && (
            <ul className="space-y-2 text-xs">
              {deposits.slice(0, 5).map((d) => (
                <li key={d.tx_hash} className="rounded-xl bg-panel-2 px-3 py-2">
                  +{formatMoney(d.rub_credit_minor)} ← {formatUsdt(d.usdt_minor)} USDT · {d.status}
                </li>
              ))}
            </ul>
          )}
        </Card>
      ) : (
        <Card>
          <p className="text-sm text-fog">Доступно {formatMoney(rubBal)}</p>
          <form className="mt-4 space-y-4" onSubmit={withdraw}>
            <div>
              <Label>Сумма ₽</Label>
              <Input value={rubAmount} onChange={(e) => setRubAmount(e.target.value)} required />
            </div>
            <div>
              <Label>Адрес TRON</Label>
              <Input
                value={toAddress}
                onChange={(e) => setToAddress(e.target.value)}
                placeholder="T…"
                required
              />
            </div>
            {withdrawQuote && (
              <p className="text-sm text-mint">
                К выводу ≈ {formatUsdt(withdrawQuote.usdt_out_minor)} USDT
                {withdrawQuote.fee_minor > 0
                  ? ` · комиссия ${formatMoney(withdrawQuote.fee_minor)}`
                  : ''}
              </p>
            )}
            <Button className="w-full" type="submit" disabled={busy}>
              {busy ? 'Оформление…' : 'Вывести USDT'}
            </Button>
          </form>
        </Card>
      )}
      {error && <p className="text-sm text-pink">{error}</p>}
      {msg && <p className="text-sm text-mint">{msg}</p>}
    </div>
  )
}

function kycStatusMeta(status: string): { label: string; hint: string; tone: string } {
  switch (status) {
    case 'draft':
      return {
        label: 'Черновик',
        hint: 'Загрузите документы и отправьте заявку на проверку.',
        tone: 'bg-panel-2 text-fog',
      }
    case 'pending':
      return {
        label: 'На проверке',
        hint: 'Заявка на проверке. Обычно ответ в течение рабочего дня.',
        tone: 'bg-pink/15 text-pink',
      }
    case 'approved':
      return {
        label: 'Подтверждена',
        hint: 'Личность подтверждена. Лимиты сняты.',
        tone: 'bg-mint/15 text-mint',
      }
    case 'rejected':
      return {
        label: 'Отклонена',
        hint: 'Исправьте замечания и загрузите документы заново.',
        tone: 'bg-pink/20 text-pink',
      }
    case 'resubmit':
      return {
        label: 'Нужна доработка',
        hint: 'Добавьте или замените документы и отправьте снова.',
        tone: 'bg-pink/15 text-pink',
      }
    default:
      return { label: status, hint: '', tone: 'bg-panel-2 text-fog' }
  }
}

const KYC_DOC_TYPES: { value: string; label: string; hint: string }[] = [
  { value: 'passport', label: 'Паспорт', hint: 'Разворот с фото' },
  { value: 'selfie', label: 'Селфи с паспортом', hint: 'Лицо и документ в кадре' },
  { value: 'address_proof', label: 'Подтверждение адреса', hint: 'Счёт / выписка' },
  { value: 'other', label: 'Другое', hint: 'По запросу банка' },
]

function docTypeLabel(value: string) {
  return KYC_DOC_TYPES.find((d) => d.value === value)?.label ?? value
}

function KycPage() {
  const [status, setStatus] = useState('draft')
  const [note, setNote] = useState('')
  const [docs, setDocs] = useState<{ id: string; doc_type: string; byte_size: number }[]>([])
  const [uploadsEnabled, setUploadsEnabled] = useState(true)
  const [docType, setDocType] = useState('passport')
  const [file, setFile] = useState<File | null>(null)
  const [msg, setMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  const canEdit = status === 'draft' || status === 'resubmit'
  const meta = kycStatusMeta(status)
  const hasPassport = docs.some((d) => d.doc_type === 'passport')
  const hasSelfie = docs.some((d) => d.doc_type === 'selfie')
  const canSubmit = canEdit && docs.length > 0 && uploadsEnabled

  const load = useCallback(async () => {
    const r = await api<{
      case: { status: string; note?: string }
      documents: { id: string; doc_type: string; byte_size: number }[]
      uploads_enabled: boolean
    }>('GET', '/api/kyc')
    setStatus(r.case.status)
    setNote(r.case.note?.trim() || '')
    setDocs(r.documents)
    setUploadsEnabled(r.uploads_enabled)
    if (!r.uploads_enabled) {
      setError('Загрузка документов временно недоступна. Попробуйте позже.')
    }
  }, [])

  useEffect(() => {
    void load().catch((e) => setError(userFacingError(e)))
  }, [load])

  async function upload(e: React.FormEvent) {
    e.preventDefault()
    if (!file || !canEdit) return
    setBusy(true)
    setError(null)
    setMsg(null)
    try {
      const fd = new FormData()
      fd.append('doc_type', docType)
      fd.append('file', file)
      const res = await fetch('/api/kyc/documents', {
        method: 'POST',
        body: fd,
        credentials: 'same-origin',
      })
      const data = (await res.json().catch(() => ({}))) as { error?: string }
      if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`)
      setMsg(`${docTypeLabel(docType)} загружен`)
      setFile(null)
      if (fileRef.current) fileRef.current.value = ''
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  async function submit() {
    setBusy(true)
    setError(null)
    setMsg(null)
    try {
      await api('POST', '/api/kyc')
      setMsg('Заявка отправлена на проверку')
      await load()
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="animate-rise space-y-4">
      <div>
        <h2 className="font-display text-2xl font-semibold">Верификация</h2>
        <p className="mt-1 text-sm text-fog">
          Подтверждение личности для снятия лимитов. Документы шифруются перед сохранением.
        </p>
      </div>

      <Card className="space-y-4">
        <div className="flex flex-wrap items-center gap-3">
          <span className={`rounded-full px-3 py-1 text-xs font-semibold ${meta.tone}`}>
            {meta.label}
          </span>
          {status === 'approved' && <CheckCircle2 className="h-5 w-5 text-mint" />}
        </div>
        <p className="text-sm leading-relaxed text-fog">{meta.hint}</p>
        {note && (
          <div className="rounded-2xl border border-line bg-panel-2 px-4 py-3 text-sm">
            <p className="text-xs font-semibold uppercase tracking-wide text-fog">Комментарий банка</p>
            <p className="mt-1 text-snow">{note}</p>
          </div>
        )}

        <div className="grid gap-2 sm:grid-cols-2">
          {[
            { ok: hasPassport, label: 'Паспорт', need: true },
            { ok: hasSelfie, label: 'Селфи с паспортом', need: true },
            {
              ok: docs.some((d) => d.doc_type === 'address_proof'),
              label: 'Адрес (по желанию)',
              need: false,
            },
          ].map((item) => (
            <div
              key={item.label}
              className="flex items-center gap-3 rounded-2xl border border-line/80 bg-panel-2/60 px-3 py-3"
            >
              <span
                className={`flex h-8 w-8 items-center justify-center rounded-full ${
                  item.ok ? 'bg-mint/15 text-mint' : 'bg-void text-fog'
                }`}
              >
                {item.ok ? <CheckCircle2 className="h-4 w-4" /> : <FileText className="h-4 w-4" />}
              </span>
              <div>
                <p className="text-sm font-medium">{item.label}</p>
                <p className="text-xs text-fog">
                  {item.ok ? 'Загружено' : item.need ? 'Обязательно' : 'Необязательно'}
                </p>
              </div>
            </div>
          ))}
        </div>
      </Card>

      {docs.length > 0 && (
        <Card className="space-y-3">
          <h3 className="font-medium">Загруженные файлы</h3>
          <ul className="space-y-2">
            {docs.map((d) => (
              <li
                key={d.id}
                className="flex items-center justify-between gap-3 rounded-2xl bg-panel-2 px-4 py-3 text-sm"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <FileText className="h-4 w-4 shrink-0 text-fog" />
                  <span className="truncate font-medium">{docTypeLabel(d.doc_type)}</span>
                </div>
                <span className="shrink-0 text-xs text-fog">
                  {(d.byte_size / 1024).toFixed(0)} КБ
                </span>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {canEdit && (
        <Card>
          <h3 className="font-medium">Загрузить документ</h3>
          <p className="mt-1 text-sm text-fog">Фото или PDF, до 10 МБ.</p>
          <form className="mt-5 space-y-4" onSubmit={upload}>
            <div>
              <Label htmlFor="kyc-type">Тип документа</Label>
              <select
                id="kyc-type"
                value={docType}
                onChange={(e) => setDocType(e.target.value)}
                className="flex h-12 w-full rounded-2xl border border-line bg-panel px-4 text-sm text-snow focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pink/40"
              >
                {KYC_DOC_TYPES.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label} — {t.hint}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <Label>Файл</Label>
              <input
                ref={fileRef}
                type="file"
                accept="image/*,.pdf,application/pdf"
                className="sr-only"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
              <button
                type="button"
                onClick={() => fileRef.current?.click()}
                className="flex w-full flex-col items-center justify-center gap-2 rounded-2xl border border-dashed border-line bg-panel-2/50 px-4 py-8 text-center transition hover:border-pink/40 hover:bg-panel-2"
              >
                <Upload className="h-6 w-6 text-pink" />
                {file ? (
                  <>
                    <span className="text-sm font-medium text-snow">{file.name}</span>
                    <span className="text-xs text-fog">
                      {(file.size / 1024).toFixed(0)} КБ · нажмите, чтобы заменить
                    </span>
                  </>
                ) : (
                  <>
                    <span className="text-sm font-medium text-snow">Выберите файл</span>
                    <span className="text-xs text-fog">JPG, PNG или PDF</span>
                  </>
                )}
              </button>
            </div>

            {error && (
              <p className="rounded-xl bg-pink/10 px-3 py-2 text-sm text-pink" role="alert">
                {error}
              </p>
            )}
            {msg && <p className="text-sm text-mint">{msg}</p>}

            <Button type="submit" className="w-full" disabled={!file || busy || !uploadsEnabled}>
              <Upload className="h-4 w-4" />
              {busy ? 'Загрузка…' : 'Загрузить'}
            </Button>
            <Button
              type="button"
              variant="mint"
              className="w-full"
              disabled={!canSubmit || busy}
              onClick={() => void submit()}
            >
              <ShieldCheck className="h-4 w-4" />
              Отправить на проверку
            </Button>
            {!hasPassport || !hasSelfie ? (
              <p className="text-center text-xs text-fog">
                Рекомендуем паспорт и селфи — так проверка проходит быстрее.
              </p>
            ) : null}
          </form>
        </Card>
      )}

      {!canEdit && status === 'pending' && (
        <Card>
          <p className="text-sm text-fog">
            Документы приняты. Менять заявку нельзя, пока идёт проверка.
          </p>
          {msg && <p className="mt-2 text-sm text-mint">{msg}</p>}
        </Card>
      )}

      {!canEdit && status === 'approved' && (
        <Card className="flex items-center gap-3">
          <ShieldCheck className="h-8 w-8 text-mint" />
          <div>
            <p className="font-medium">Вы верифицированы</p>
            <p className="text-sm text-fog">Можно пользоваться банком без дополнительных лимитов.</p>
          </div>
        </Card>
      )}
    </div>
  )
}

function AppGate() {
  const nav = useNavigate()
  const { me, error, loading, refresh } = useBankMe()
  const authed = !loading && !!me && !error
  const { status, unread, setUnread } = useSessionWatch(authed)

  useEffect(() => {
    const onLost = () => {
      resetSecureSession()
      nav('/auth', { replace: true })
    }
    const onBeat = () => {
      /* balance/list refresh stays page-local; unread comes from watch */
    }
    window.addEventListener('laefye:session-lost', onLost)
    window.addEventListener('laefye:heartbeat', onBeat)
    return () => {
      window.removeEventListener('laefye:session-lost', onLost)
      window.removeEventListener('laefye:heartbeat', onBeat)
    }
  }, [nav])

  useEffect(() => {
    if (status === 'auth') {
      resetSecureSession()
      nav('/auth', { replace: true })
    }
  }, [status, nav])

  async function logout() {
    try {
      await api('POST', '/api/logout')
    } catch {
      /* ignore */
    }
    resetSecureSession()
    nav('/auth')
  }

  if (loading) {
    return (
      <Shell>
        <p className="text-fog">Загрузка…</p>
      </Shell>
    )
  }

  if (error || !me) {
    return <Navigate to="/auth" replace />
  }

  if (!me.profile) {
    return (
      <Shell
        username={me.username}
        onLogout={logout}
        linkStatus={status}
        unread={unread}
        onUnread={setUnread}
      >
        <Onboarding onDone={() => void refresh()} />
      </Shell>
    )
  }

  return (
    <Shell
      username={`@${me.profile.handle}`}
      onLogout={logout}
      linkStatus={status}
      unread={unread}
      onUnread={setUnread}
    >
      <Routes>
        <Route index element={<Dashboard me={me} refresh={() => void refresh()} />} />
        <Route path="transfer" element={<TransferPage me={me} onDone={() => void refresh()} />} />
        <Route path="crypto" element={<CryptoPage me={me} onDone={() => void refresh()} />} />
        <Route path="fx" element={<RatesPage />} />
        <Route path="history" element={<HistoryPage me={me} />} />
        <Route path="account" element={<AccountPage me={me} onDone={() => void refresh()} />} />
        <Route path="docs" element={<DocsPage me={me} />} />
        <Route path="kyc" element={<KycPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
    </Shell>
  )
}

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Landing />} />
      <Route path="/auth" element={<AuthPage />} />
      <Route path="/app/*" element={<AppGate />} />
      <Route path="*" element={<NotFoundPage />} />
    </Routes>
  )
}
