import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Fingerprint } from 'lucide-react'
import { Logo } from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { Card, Input, Label } from '@/components/ui/input'
import { loginPasskey, registerPasskey } from '@/lib/api'
import { userFacingError } from '@/lib/utils'
import { getVisitorId } from '@/lib/device'

/** Drop autofill junk like the literal string "null". */
function sanitizeUsername(raw: string): string {
  const u = raw.trim()
  if (!u || u.toLowerCase() === 'null' || u.toLowerCase() === 'undefined') return ''
  return u
}

export default function AuthPage() {
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
      const user = sanitizeUsername(username)
      if (mode === 'register') {
        if (!user) throw new Error('Укажите имя пользователя')
        await registerPasskey(user, displayName.trim() || undefined)
      } else {
        await loginPasskey(user || undefined)
      }
      nav('/app', { viewTransition: true })
    } catch (err) {
      setError(userFacingError(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-auto flex min-h-dvh w-full max-w-lg flex-col justify-center px-6 py-10">
      <Logo to="/" size="md" className="mb-8" />
      <Card className="route-enter">
        <h2 className="font-display text-2xl font-semibold">
          {mode === 'register' ? 'Открытие счёта' : 'Вход'}
        </h2>
        <p className="mt-2 text-sm text-fog">
          {mode === 'register'
            ? 'Имя пользователя станет вашим @ником для переводов. Passkey — вместо пароля.'
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
            <Label htmlFor="user">{mode === 'register' ? '@ник' : 'Имя пользователя'}</Label>
            <Input
              id="user"
              autoComplete="username webauthn"
              value={username}
              onChange={(e) =>
                setUsername(
                  mode === 'register' ? e.target.value.toLowerCase() : e.target.value,
                )
              }
              placeholder="username"
              required={mode === 'register'}
            />
            {mode === 'register' && (
              <p className="mt-1 text-xs text-fog">По нему вам будут переводить деньги.</p>
            )}
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
            {busy ? 'Ожидание ключа…' : mode === 'register' ? 'Создать passkey' : 'Войти с passkey'}
          </Button>
        </form>
      </Card>
    </div>
  )
}
