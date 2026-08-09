import { useId } from 'react'
import { PrefetchLink } from '@/components/PrefetchLink'
import { cn } from '@/lib/utils'

/** Brand seal: pink mass + mint satellite with void core — unique twin-orbit, not a period. */
export function BrandMark({ className, animated }: { className?: string; animated?: boolean }) {
  const uid = useId().replace(/:/g, '')
  const maskId = `laefye-orbit-${uid}`

  return (
    <svg
      viewBox="0 0 28 28"
      aria-hidden
      className={cn(
        'inline-block shrink-0 overflow-visible',
        animated && 'brand-mark-pulse',
        className,
      )}
    >
      <defs>
        <mask id={maskId}>
          <rect width="28" height="28" fill="#fff" />
          <circle cx="18.2" cy="14.5" r="6.6" fill="#000" />
        </mask>
      </defs>
      <circle cx="11.2" cy="14.5" r="8.4" fill="#ff3d7f" mask={`url(#${maskId})`} />
      <circle cx="18.2" cy="14.5" r="5.6" fill="#3dffb5" />
      <circle cx="18.2" cy="14.5" r="2" fill="#070708" />
    </svg>
  )
}

type LogoProps = {
  to?: string
  size?: 'sm' | 'md' | 'lg' | 'hero'
  className?: string
  animated?: boolean
  as?: 'link' | 'span'
}

const sizeClass = {
  sm: 'text-lg gap-1',
  md: 'text-2xl gap-1.5',
  lg: 'text-3xl gap-2',
  hero: 'text-4xl gap-2.5 sm:text-5xl',
} as const

const markSize = {
  sm: 'h-[0.72em] w-[0.72em]',
  md: 'h-[0.78em] w-[0.78em]',
  lg: 'h-[0.82em] w-[0.82em]',
  hero: 'h-[0.85em] w-[0.85em]',
} as const

export function Logo({
  to = '/',
  size = 'md',
  className,
  animated = false,
  as = 'link',
}: LogoProps) {
  const body = (
    <>
      <span className="tracking-tight">laefye</span>
      <BrandMark className={cn(markSize[size], 'translate-y-[0.06em]')} animated={animated} />
    </>
  )

  const cls = cn(
    'inline-flex items-center font-display font-semibold leading-none text-snow',
    sizeClass[size],
    className,
  )

  if (as === 'span') {
    return (
      <span className={cls} aria-label="laefye">
        {body}
      </span>
    )
  }

  return (
    <PrefetchLink to={to} className={cls} aria-label="laefye">
      {body}
    </PrefetchLink>
  )
}
