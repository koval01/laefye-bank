import { useCallback, useEffect, useRef, useState } from 'react'
import { api, ApiError } from '@/lib/api'

export type LinkStatus = 'online' | 'degraded' | 'offline' | 'auth'

type Heartbeat = {
  ok: boolean
  server_time: string
  session_expires_at: string
  unread: number
  idle_secs: number
}

const VISIBLE_MS = 15_000
const HIDDEN_MS = 45_000

/**
 * Keep-alive + unread counter. Continues while the tab is backgrounded
 * (browsers may throttle timers; we also listen to visibility/focus/online).
 */
export function useSessionWatch(enabled: boolean) {
  const [status, setStatus] = useState<LinkStatus>('online')
  const [unread, setUnread] = useState(0)
  const [lastOkAt, setLastOkAt] = useState<number | null>(null)
  const timer = useRef<number | null>(null)
  const tickGen = useRef(0)

  const clearTimer = () => {
    if (timer.current != null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
  }

  const beat = useCallback(async () => {
    if (!enabled) return
    const gen = ++tickGen.current
    try {
      const data = await api<Heartbeat>('GET', '/api/session/heartbeat')
      if (gen !== tickGen.current) return
      setUnread(data.unread ?? 0)
      setLastOkAt(Date.now())
      setStatus('online')
      window.dispatchEvent(
        new CustomEvent('laefye:heartbeat', { detail: data }),
      )
    } catch (err) {
      if (gen !== tickGen.current) return
      if (err instanceof ApiError && err.status === 401) {
        setStatus('auth')
        window.dispatchEvent(new CustomEvent('laefye:session-lost'))
        return
      }
      setStatus((prev) => (prev === 'offline' ? 'offline' : 'degraded'))
    }
  }, [enabled])

  const schedule = useCallback(() => {
    clearTimer()
    if (!enabled) return
    const ms = document.visibilityState === 'hidden' ? HIDDEN_MS : VISIBLE_MS
    timer.current = window.setTimeout(() => {
      void beat().finally(() => schedule())
    }, ms)
  }, [beat, enabled])

  useEffect(() => {
    if (!enabled) {
      clearTimer()
      return
    }
    void beat().finally(() => schedule())

    const onVis = () => {
      if (document.visibilityState === 'visible') {
        void beat()
        schedule()
      } else {
        schedule()
      }
    }
    const onOnline = () => {
      setStatus('degraded')
      void beat()
    }
    const onOffline = () => setStatus('offline')
    const onFocus = () => void beat()

    document.addEventListener('visibilitychange', onVis)
    window.addEventListener('online', onOnline)
    window.addEventListener('offline', onOffline)
    window.addEventListener('focus', onFocus)

    return () => {
      clearTimer()
      document.removeEventListener('visibilitychange', onVis)
      window.removeEventListener('online', onOnline)
      window.removeEventListener('offline', onOffline)
      window.removeEventListener('focus', onFocus)
    }
  }, [enabled, beat, schedule])

  return { status, unread, lastOkAt, refresh: beat, setUnread }
}
