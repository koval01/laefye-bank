import * as React from 'react'
import { cn } from '@/lib/utils'

export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      className={cn(
        'flex h-12 w-full rounded-2xl border border-line bg-panel px-4 text-sm text-snow placeholder:text-fog/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pink/40',
        className,
      )}
      {...props}
    />
  ),
)
Input.displayName = 'Input'

export function Label({ className, ...props }: React.LabelHTMLAttributes<HTMLLabelElement>) {
  return (
    <label
      className={cn('mb-2 block text-[11px] font-semibold uppercase tracking-[0.14em] text-fog', className)}
      {...props}
    />
  )
}

export function Card({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn('card-shine rounded-[1.35rem] border border-line/80 p-5', className)}
      {...props}
    />
  )
}
