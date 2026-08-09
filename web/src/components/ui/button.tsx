import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 rounded-2xl text-sm font-semibold transition active:scale-[0.98] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pink/50 disabled:pointer-events-none disabled:opacity-45',
  {
    variants: {
      variant: {
        default: 'bg-pink text-snow hover:bg-pink-deep',
        outline: 'border border-line bg-panel text-snow hover:border-pink/40 hover:bg-panel-2',
        ghost: 'text-fog hover:bg-panel-2 hover:text-snow',
        mint: 'bg-mint text-void hover:brightness-110',
        soft: 'bg-panel-2 text-snow hover:bg-line',
      },
      size: {
        default: 'h-12 px-5',
        sm: 'h-9 px-3 text-xs',
        lg: 'h-14 px-7 text-base',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button ref={ref} className={cn(buttonVariants({ variant, size }), className)} {...props} />
  ),
)
Button.displayName = 'Button'
