import { useEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'

/** Full-viewport overlay. Portaled to body so page transforms cannot clip it. */
export function ModalShell({
  children,
  className,
  onBackdrop,
  zClass = 'z-50',
}: {
  children: ReactNode
  className?: string
  onBackdrop?: () => void
  zClass?: string
}) {
  useEffect(() => {
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = prev
    }
  }, [])

  useEffect(() => {
    if (!onBackdrop) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onBackdrop()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onBackdrop])

  return createPortal(
    <div
      role="presentation"
      className={cn(
        'fixed inset-0 flex items-end justify-center bg-void/75 p-3 sm:items-center sm:p-4',
        zClass,
      )}
      onClick={(e) => {
        if (e.target === e.currentTarget) onBackdrop?.()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        className={cn(
          'max-h-[min(92dvh,920px)] w-full max-w-lg overflow-y-auto overscroll-contain',
          className,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  )
}
