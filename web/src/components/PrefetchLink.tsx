import { forwardRef, useCallback, type ComponentPropsWithoutRef } from 'react'
import { Link, NavLink, type NavLinkRenderProps } from 'react-router-dom'
import { prefetch } from '@/lib/prefetch'

type PrefetchMode = 'intent' | 'render' | 'none'

type Common = {
  prefetch?: PrefetchMode
  viewTransition?: boolean
}

type PrefetchLinkProps = ComponentPropsWithoutRef<typeof Link> & Common

function useWarm(to: PrefetchLinkProps['to'], mode: PrefetchMode) {
  const href = typeof to === 'string' ? to : to.pathname || '/'
  return useCallback(() => {
    if (mode === 'none') return
    prefetch(href)
  }, [href, mode])
}

/** SPA link: no full reload, View Transition, chunk/API prefetch on intent. */
export const PrefetchLink = forwardRef<HTMLAnchorElement, PrefetchLinkProps>(
  function PrefetchLink(
    { prefetch: mode = 'intent', viewTransition = true, onPointerEnter, onFocus, to, ...rest },
    ref,
  ) {
    const warm = useWarm(to, mode)
    if (mode === 'render') warm()

    return (
      <Link
        ref={ref}
        to={to}
        viewTransition={viewTransition}
        onPointerEnter={(e) => {
          warm()
          onPointerEnter?.(e)
        }}
        onFocus={(e) => {
          warm()
          onFocus?.(e)
        }}
        {...rest}
      />
    )
  },
)

type PrefetchNavLinkProps = Omit<ComponentPropsWithoutRef<typeof NavLink>, 'className'> &
  Common & {
    className?: string | ((props: NavLinkRenderProps) => string | undefined)
  }

export const PrefetchNavLink = forwardRef<HTMLAnchorElement, PrefetchNavLinkProps>(
  function PrefetchNavLink(
    { prefetch: mode = 'intent', viewTransition = true, onPointerEnter, onFocus, to, ...rest },
    ref,
  ) {
    const warm = useWarm(to, mode)
    if (mode === 'render') warm()

    return (
      <NavLink
        ref={ref}
        to={to}
        viewTransition={viewTransition}
        onPointerEnter={(e) => {
          warm()
          onPointerEnter?.(e)
        }}
        onFocus={(e) => {
          warm()
          onFocus?.(e)
        }}
        {...rest}
      />
    )
  },
)
