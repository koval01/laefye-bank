import { useCallback, useEffect, useState } from 'react'
import { Bell } from 'lucide-react'
import { Link } from 'react-router-dom'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { cn, formatDateTime } from '@/lib/utils'

type Notif = {
  id: string
  kind: string
  title: string
  body: string
  href?: string | null
  read_at?: string | null
  created_at: string
}

export function NotificationBell({
  unread,
  onUnread,
}: {
  unread: number
  onUnread: (n: number) => void
}) {
  const [open, setOpen] = useState(false)
  const [items, setItems] = useState<Notif[]>([])
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    const r = await api<{ notifications: Notif[]; unread: number }>(
      'GET',
      '/api/notifications?limit=20',
    )
    setItems(r.notifications)
    onUnread(r.unread)
  }, [onUnread])

  useEffect(() => {
    if (!open) return
    void load().catch(() => undefined)
  }, [open, load])

  async function markAll() {
    setBusy(true)
    try {
      await api('POST', '/api/notifications/read-all')
      await load()
    } finally {
      setBusy(false)
    }
  }

  async function markOne(id: string) {
    await api('POST', `/api/notifications/${id}/read`)
    await load()
  }

  return (
    <div className="relative">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        aria-label="Уведомления"
        className="relative"
        onClick={() => setOpen((v) => !v)}
      >
        <Bell className="h-4 w-4" />
        {unread > 0 && (
          <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-pink px-1 text-[10px] font-bold text-white">
            {unread > 9 ? '9+' : unread}
          </span>
        )}
      </Button>
      {open && (
        <>
          <button
            type="button"
            className="fixed inset-0 z-40 cursor-default"
            aria-label="Закрыть"
            onClick={() => setOpen(false)}
          />
          <div className="absolute right-0 z-50 mt-2 w-[min(92vw,22rem)] overflow-hidden rounded-2xl border border-line bg-panel shadow-2xl">
            <div className="flex items-center justify-between gap-2 border-b border-line px-4 py-3">
              <p className="font-medium">Уведомления</p>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy || unread === 0}
                onClick={() => void markAll()}
              >
                Прочитать все
              </Button>
            </div>
            <ul className="max-h-[min(60dvh,24rem)] overflow-y-auto">
              {items.map((n) => (
                <li
                  key={n.id}
                  className={cn(
                    'border-b border-line/60 px-4 py-3 text-sm last:border-0',
                    !n.read_at && 'bg-panel-2/60',
                  )}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    onClick={() => void markOne(n.id)}
                  >
                    <p className="font-medium">{n.title}</p>
                    {n.body && <p className="mt-0.5 text-xs text-fog">{n.body}</p>}
                    <p className="mt-1 text-[11px] text-fog">{formatDateTime(n.created_at)}</p>
                  </button>
                  {n.href && (
                    <Link
                      to={n.href}
                      className="mt-1 inline-block text-xs text-pink"
                      onClick={() => setOpen(false)}
                    >
                      Открыть
                    </Link>
                  )}
                </li>
              ))}
              {!items.length && (
                <li className="px-4 py-8 text-center text-sm text-fog">Пока тихо</li>
              )}
            </ul>
          </div>
        </>
      )}
    </div>
  )
}
