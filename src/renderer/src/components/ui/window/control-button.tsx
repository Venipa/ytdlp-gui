import { cn } from '@renderer/lib/utils'
import { cva, type VariantProps } from 'class-variance-authority'
import { Minus, Square, SquareArrowOutDownLeftIcon, X } from 'lucide-react'
import * as React from 'react'

const controlButtonVariants = cva(
  'inline-flex items-center justify-center rounded-sm text-sm font-medium ring-offset-background transition-all focus-visible:outline-none scale-100 active:scale-[.9465] disabled:pointer-events-none disabled:opacity-50 relative overflow-hidden',
  {
    variants: {
      variant: {
        default: 'hover:bg-secondary hover:text-secondary-foreground',
        close: 'hover:bg-destructive/90 hover:text-destructive-foreground',
        transparent: 'hover:bg-primary/10 hover:text-primary'
      },
      size: {
        default: 'size-7'
      }
    },
    defaultVariants: {
      variant: 'default',
      size: 'default'
    }
  }
)

export interface ControlButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof controlButtonVariants> {
  icon: 'minimize' | 'maximize' | 'maximize-out' | 'close'
}

const ControlButton = React.forwardRef<HTMLButtonElement, ControlButtonProps>(
  ({ className, variant, size, icon, ...props }, ref) => {
    const Icon =
      icon === 'minimize'
        ? Minus
        : icon === 'maximize'
          ? Square
          : icon === 'maximize-out'
            ? SquareArrowOutDownLeftIcon
            : X

    return (
      <button
        className={cn(controlButtonVariants({ variant, size, className }), 'group')}
        ref={ref}
        {...props}
      >
        <Icon
          className={cn(
            'size-3 relative z-10',
            icon === 'minimize' && 'self-end mb-1',
            icon === 'close' && 'size-3.5'
          )}
        />
      </button>
    )
  }
)
ControlButton.displayName = 'ControlButton'

export { ControlButton, controlButtonVariants }
