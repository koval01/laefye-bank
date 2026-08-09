import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useLocation, useNavigationType } from 'react-router-dom'
import { cn } from '@/lib/utils'

/**
 * Soft route stage animation when View Transitions API is unavailable.
 * With VT enabled (PrefetchLink / navigate viewTransition), the browser
 * handles the cross-fade; this still keys content for a clean mount.
 */
export function RouteStage({ children, className }: { children: ReactNode; className?: string }) {
  const location = useLocation()
  const navType = useNavigationType()
  const [tick, setTick] = useState(0)
  const reduce = useRef(false)

  useEffect(() => {
    reduce.current = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  }, [])

  useEffect(() => {
    setTick((t) => t + 1)
  }, [location.pathname, location.search])

  const useCssEnter =
    typeof document !== 'undefined' &&
    !('startViewTransition' in document) &&
    !reduce.current &&
    navType !== 'POP'

  return (
    <div
      key={`${location.pathname}?${location.search}:${tick}`}
      className={cn(useCssEnter && 'route-enter', className)}
    >
      {children}
    </div>
  )
}
