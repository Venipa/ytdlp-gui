import { cva, type VariantProps } from 'class-variance-authority'
import * as React from 'react'

import { cn } from '@renderer/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center rounded-md border font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 select-none cursor-default',
  {
    variants: {
      variant: {
        default: 'border-transparent bg-primary text-primary-foreground shadow hover:bg-primary/80',
        secondary:
          'border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80',
        destructive:
          'border-destructive bg-destructive/20 text-destructive-foreground shadow hover:bg-destructive/30',
        outline: 'text-foreground'
      },
      size: {
        default: 'px-2.5 py-0.5 text-xs gap-2 [&_svg]:size-4',
        sm: 'px-1.5 py-0 text-xs gap-1.5 [&_svg]:size-4'
      }
    },
    defaultVariants: {
      variant: 'default',
      size: 'default'
    }
  }
)

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />
}

export { Badge, badgeVariants }
