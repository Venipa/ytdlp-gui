import { Slot } from '@radix-ui/react-slot'
import { cn } from '@renderer/lib/utils'
import { cva, VariantProps } from 'class-variance-authority'
import { PropsWithChildren } from 'react'

const textVariants = cva('whitespace-nowrap cursor-default', {
  variants: {
    variant: {
      default: 'text-black font-medium dark:text-muted-foreground opacity-80 hover:opacity-100'
    },
    size: {
      sm: 'text-sm',
      default: 'text-xs',
      lg: 'text-lg'
    }
  },
  defaultVariants: {
    variant: 'default',
    size: 'default'
  }
})
export interface ButtonTextProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof textVariants> {
  asChild?: boolean
}
export default function ClickableText({
  children,
  variant,
  className,
  onClick,
  asChild,
  ...props
}: PropsWithChildren<ButtonTextProps>) {
  const Comp = asChild ? Slot : 'button'
  return (
    <Comp
      type="button"
      className={textVariants({
        className: cn(onClick && 'cursor-pointer', className),
        variant
      })}
      onClick={(ev) => {
        if (props.disabled) {
          ev.preventDefault()
          ev.stopPropagation()
          return
        }
        onClick?.(ev)
      }}
      {...props}
    >
      {children}
    </Comp>
  )
}
