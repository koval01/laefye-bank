import type { ReactNode } from 'react'
import { Link, NavLink } from 'react-router-dom'
import { Home, Receipt, Send, UserRound, Wallet } from 'lucide-react'
import { Logo } from '@/components/Logo'
import { NotificationBell } from '@/components/NotificationBell'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { LinkStatus } from '@/lib/session-watch'

const links: { to: string; label: string; icon: typeof Home; end?: boolean }[] = [
  { to: '/app', label: 'Главная', icon: Home, end: true },
  { to: '/app/transfer', label: 'Перевод', icon: Send },
  { to: '/app/crypto', label: 'USDT', icon: Wallet },
  { to: '/app/history', label: 'Движения', icon: Receipt },
  { to: '/app/account', label: 'Аккаунт', icon: UserRound },
]

function linkLabel(status: LinkStatus): string {
  switch (status) {
    case 'online':
      return 'Связь OK'
    case 'degraded':
      return 'Связь слабая'
    case 'offline':
      return 'Нет сети'
    case 'auth':
      return 'Сессия'
  }
}

export function Shell({
  children,
  username,
  onLogout,
  linkStatus = 'online',
  unread = 0,
  onUnread,
}: {
  children: ReactNode
  username?: string
  onLogout?: () => void
  linkStatus?: LinkStatus
  unread?: number
  onUnread?: (n: number) => void
}) {
  return (
    <div className="mx-auto flex min-h-dvh w-full min-w-0 max-w-3xl flex-col px-4 pb-28 pt-5 sm:px-6 lg:max-w-5xl lg:pb-8 lg:pt-8">
      <header className="mb-6 flex items-center justify-between gap-3">
        <Logo to="/app" size="md" />
        <div className="flex items-center gap-1.5 sm:gap-2">
          <span
            className={cn(
              'hidden rounded-full px-2.5 py-1 text-[10px] font-medium sm:inline',
              linkStatus === 'online' && 'bg-mint/15 text-mint',
              linkStatus === 'degraded' && 'bg-panel-2 text-fog',
              linkStatus === 'offline' && 'bg-pink/15 text-pink',
              linkStatus === 'auth' && 'bg-pink/15 text-pink',
            )}
            title={linkLabel(linkStatus)}
          >
            {linkLabel(linkStatus)}
          </span>
          {onUnread && <NotificationBell unread={unread} onUnread={onUnread} />}
          {username && (
            <Link
              to="/app/account"
              className="max-w-[10rem] truncate rounded-full bg-panel-2 px-3 py-1 text-xs text-fog transition hover:text-snow"
            >
              {username}
            </Link>
          )}
          {onLogout && (
            <Button variant="ghost" size="sm" onClick={onLogout}>
              Выйти
            </Button>
          )}
        </div>
      </header>

      <main className="flex-1">{children}</main>

      <nav className="fixed inset-x-0 bottom-0 z-20 border-t border-line/70 bg-void/95 backdrop-blur-xl lg:static lg:mt-10 lg:border-t-0 lg:bg-transparent lg:backdrop-blur-none">
        <div className="mx-auto grid max-w-3xl grid-cols-5 gap-1 px-2 py-2 lg:max-w-5xl lg:rounded-2xl lg:border lg:border-line lg:bg-panel lg:px-2 lg:py-2">
          {links.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                cn(
                  'flex flex-col items-center gap-1 rounded-xl px-1 py-2 text-[10px] font-medium text-fog transition lg:flex-row lg:justify-center lg:gap-2 lg:text-xs',
                  isActive && 'bg-panel-2 text-pink lg:bg-pink/15',
                )
              }
            >
              <Icon className="h-5 w-5" strokeWidth={2.25} />
              {label}
            </NavLink>
          ))}
        </div>
      </nav>
    </div>
  )
}
