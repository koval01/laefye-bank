import { useEffect, useRef } from 'react'
import { Fingerprint, AtSign, Coins, ArrowDown } from 'lucide-react'
import { Logo } from '@/components/Logo'
import { PrefetchLink } from '@/components/PrefetchLink'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * Multi-layer scroll parallax landing.
 * First viewport: brand-dominant (laefye), one line, CTAs, full-bleed atmosphere.
 */
export default function Landing() {
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const root = rootRef.current
    if (!root || reduced) return

    const layers = root.querySelectorAll<HTMLElement>('[data-parallax]')
    let raf = 0
    let latest = window.scrollY

    const paint = () => {
      raf = 0
      const y = latest
      const vh = window.innerHeight
      for (const el of layers) {
        const speed = Number(el.dataset.parallax) || 0.2
        const rect = el.parentElement?.getBoundingClientRect()
        const local = rect ? -rect.top : y
        el.style.transform = `translate3d(0, ${(local * speed).toFixed(2)}px, 0)`
      }
      const hero = root.querySelector<HTMLElement>('[data-hero-fade]')
      if (hero) {
        const t = Math.min(1, Math.max(0, y / (vh * 0.55)))
        hero.style.opacity = String(1 - t * 0.85)
        hero.style.transform = `translate3d(0, ${(y * 0.18).toFixed(2)}px, 0) scale(${(1 + t * 0.06).toFixed(3)})`
      }
    }

    const onScroll = () => {
      latest = window.scrollY
      if (!raf) raf = requestAnimationFrame(paint)
    }

    paint()
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll)
    return () => {
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
      if (raf) cancelAnimationFrame(raf)
    }
  }, [])

  return (
    <div ref={rootRef} className="landing-root relative bg-void text-snow">
      <header className="landing-header fixed inset-x-0 top-0 z-40">
        <div className="mx-auto flex h-16 max-w-6xl items-center justify-between px-5 sm:h-[4.25rem] sm:px-8">
          <Logo size="md" animated as="span" />
          <nav className="flex items-center gap-2">
            <PrefetchLink to="/auth?mode=login">
              <Button variant="ghost" size="sm">
                Войти
              </Button>
            </PrefetchLink>
            <PrefetchLink to="/auth">
              <Button size="sm">Открыть счёт</Button>
            </PrefetchLink>
          </nav>
        </div>
      </header>

      <section className="relative flex min-h-dvh flex-col justify-end overflow-hidden pb-16 pt-24 sm:pb-20 sm:pt-28">
        <div className="pointer-events-none absolute inset-0" aria-hidden>
          <div
            data-parallax="0.12"
            className="absolute -left-[20%] top-[-10%] h-[70vmax] w-[70vmax] rounded-full bg-[radial-gradient(circle,rgba(255,61,127,0.28)_0%,transparent_68%)] blur-2xl will-change-transform"
          />
          <div
            data-parallax="0.22"
            className="absolute -right-[15%] top-[5%] h-[55vmax] w-[55vmax] rounded-full bg-[radial-gradient(circle,rgba(61,255,181,0.14)_0%,transparent_65%)] blur-3xl will-change-transform"
          />
          <div
            data-parallax="0.08"
            className="landing-grid absolute inset-0 opacity-[0.35] will-change-transform"
          />
          <div
            data-parallax="0.35"
            className="absolute left-[8%] top-[28%] h-24 w-24 rounded-full border border-pink/25 bg-pink/10 blur-[1px] will-change-transform sm:h-36 sm:w-36"
          />
          <div
            data-parallax="0.45"
            className="absolute right-[12%] top-[42%] h-16 w-16 rounded-full border border-mint/30 bg-mint/10 will-change-transform sm:h-24 sm:w-24"
          />
          <div
            data-parallax="0.28"
            className="absolute bottom-[22%] left-[42%] h-3 w-3 rounded-full bg-snow/40 will-change-transform"
          />
        </div>

        <div
          data-hero-fade
          className="relative z-10 mx-auto w-full max-w-6xl px-5 will-change-transform sm:px-8"
        >
          <Logo size="hero" animated as="span" className="landing-brand-in" />
          <p className="landing-brand-in mt-6 max-w-md text-base leading-relaxed text-fog delay-100 sm:text-lg">
            Счёт без паролей. Переводы по @нику. USDT на рублёвый баланс.
          </p>
          <div className="landing-brand-in mt-9 flex flex-col gap-3 delay-200 sm:flex-row sm:items-center">
            <PrefetchLink to="/auth">
              <Button size="lg" className="w-full sm:w-auto">
                Открыть счёт
              </Button>
            </PrefetchLink>
            <PrefetchLink to="/auth?mode=login">
              <Button size="lg" variant="outline" className="w-full sm:w-auto">
                Войти с passkey
              </Button>
            </PrefetchLink>
          </div>
        </div>

        <a
          href="#passkey"
          className="absolute bottom-6 left-1/2 z-10 flex -translate-x-1/2 flex-col items-center gap-1 text-[10px] font-semibold uppercase tracking-[0.2em] text-fog/80 transition hover:text-snow"
        >
          дальше
          <ArrowDown className="landing-bounce h-4 w-4" />
        </a>
      </section>

      <FeaturePlane
        id="passkey"
        tint="pink"
        parallax={0.15}
        icon={<Fingerprint className="h-7 w-7" strokeWidth={1.75} />}
        kicker="Вход"
        title="Passkey вместо пароля"
        body="Face ID, Touch ID или ключ на устройстве. Сервер не хранит секреты, которые можно украсть из формы."
      />

      <FeaturePlane
        id="transfers"
        tint="mint"
        parallax={0.18}
        icon={<AtSign className="h-7 w-7" strokeWidth={1.75} />}
        kicker="Переводы"
        title="Деньги по @нику"
        body="Как сообщение в мессенджере: нашёл человека, отправил рубли. Без реквизитов и без «перевод родственнику»."
        reverse
      />

      <FeaturePlane
        id="usdt"
        tint="mixed"
        parallax={0.2}
        icon={<Coins className="h-7 w-7" strokeWidth={1.75} />}
        kicker="Крипта"
        title="USDT → рубли на счёт"
        body="Пополнение и вывод через TRON TRC-20. Курс обновляется сам — ты видишь, сколько придёт до отправки."
      />

      <section className="relative overflow-hidden px-5 py-28 sm:px-8 sm:py-36">
        <div
          data-parallax="0.1"
          className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_70%_60%_at_50%_40%,rgba(255,61,127,0.2),transparent_70%)] will-change-transform"
          aria-hidden
        />
        <div className="relative z-10 mx-auto max-w-3xl text-center">
          <Logo size="lg" animated as="span" className="justify-center" />
          <h2 className="mt-8 font-display text-3xl font-semibold tracking-tight sm:text-4xl">
            Открой счёт за минуту
          </h2>
          <p className="mx-auto mt-4 max-w-lg text-fog">
            Без отделений, без паролей, без бумажек. Только устройство и ты.
          </p>
          <div className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <PrefetchLink to="/auth">
              <Button size="lg">Создать passkey</Button>
            </PrefetchLink>
            <PrefetchLink to="/auth?mode=login">
              <Button size="lg" variant="ghost">
                Уже есть счёт
              </Button>
            </PrefetchLink>
          </div>
        </div>
      </section>

      <footer className="border-t border-line/50 px-5 py-8 text-center text-xs text-fog sm:px-8">
        © {new Date().getFullYear()} Laefye
      </footer>
    </div>
  )
}

function FeaturePlane({
  id,
  tint,
  parallax,
  icon,
  kicker,
  title,
  body,
  reverse,
}: {
  id: string
  tint: 'pink' | 'mint' | 'mixed'
  parallax: number
  icon: React.ReactNode
  kicker: string
  title: string
  body: string
  reverse?: boolean
}) {
  const glow =
    tint === 'pink'
      ? 'rgba(255,61,127,0.2)'
      : tint === 'mint'
        ? 'rgba(61,255,181,0.14)'
        : 'rgba(255,61,127,0.12)'

  return (
    <section
      id={id}
      className="relative flex min-h-[88dvh] items-center overflow-hidden border-t border-line/40 px-5 py-24 sm:px-8 sm:py-28"
    >
      <div className="pointer-events-none absolute inset-0" aria-hidden>
        <div
          data-parallax={String(parallax)}
          className={cn(
            'absolute top-[10%] h-[50vmax] w-[50vmax] rounded-full blur-3xl will-change-transform',
            reverse ? 'right-[-10%]' : 'left-[-10%]',
          )}
          style={{ background: `radial-gradient(circle, ${glow}, transparent 70%)` }}
        />
        <div
          data-parallax={String(parallax + 0.12)}
          className={cn(
            'absolute top-1/3 h-40 w-40 rounded-[2rem] border border-line/60 bg-panel/30 will-change-transform sm:h-56 sm:w-56',
            reverse ? 'left-[8%]' : 'right-[8%]',
          )}
        />
      </div>

      <div
        className={cn(
          'relative z-10 mx-auto grid w-full max-w-6xl gap-12 lg:grid-cols-2 lg:items-center lg:gap-20',
          reverse && 'lg:[&>*:first-child]:order-2',
        )}
      >
        <div>
          <p className="text-[11px] font-semibold uppercase tracking-[0.22em] text-pink">{kicker}</p>
          <h2 className="mt-4 font-display text-3xl font-semibold tracking-tight sm:text-4xl lg:text-5xl">
            {title}
          </h2>
          <p className="mt-5 max-w-md text-base leading-relaxed text-fog sm:text-lg">{body}</p>
        </div>
        <div className="relative flex min-h-[14rem] items-center justify-center sm:min-h-[18rem]">
          <div
            data-parallax={String(parallax * 0.6)}
            className="flex h-36 w-36 items-center justify-center rounded-full border border-line/80 bg-panel/50 text-pink shadow-[0_0_80px_rgba(255,61,127,0.12)] will-change-transform sm:h-44 sm:w-44"
          >
            {icon}
          </div>
        </div>
      </div>
    </section>
  )
}
