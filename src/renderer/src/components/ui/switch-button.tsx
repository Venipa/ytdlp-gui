import { Switch as ShadcnCheckbox } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { cva, type VariantProps } from 'class-variance-authority'
import * as React from 'react'

const checkboxVariants = cva(
  'group flex items-start space-x-3 rounded-lg p-4 transition-colors cursor-pointer select-none',
  {
    variants: {
      variant: {
        default: 'bg-background hover:bg-muted',
        outline:
          'border border-input hover:border-muted-foreground/40 hover:bg-muted/60 focus-within:border-muted-foreground/40 focus-within:bg-muted/60',
        primary: 'bg-primary/10 hover:bg-primary/20 text-primary'
      }
    },
    defaultVariants: {
      variant: 'outline'
    }
  }
)

interface CheckboxProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof checkboxVariants> {
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
}

const Checkbox = React.forwardRef<HTMLDivElement, CheckboxProps>(
  ({ checked, defaultChecked, onCheckedChange, variant, className, children, ...props }, ref) => {
    const id = React.useId()
    const [internalChecked, setInternalChecked] = React.useState(defaultChecked ?? false)

    const isControlled = checked !== undefined
    const isChecked = isControlled ? checked : internalChecked

    const handleCheckedChange = (newChecked: boolean) => {
      if (!isControlled) {
        setInternalChecked(newChecked)
      }
      onCheckedChange?.(newChecked)
    }

    const handleClick = (event: React.MouseEvent<HTMLDivElement>) => {
      if (event.target instanceof HTMLInputElement) return
      handleCheckedChange(!isChecked)
    }

    return (
      <div
        ref={ref}
        onClick={handleClick}
        className={cn(checkboxVariants({ variant, className }))}
        {...props}
      >
        <div className="space-y-1 leading-none flex-auto">
          {React.Children.map(children, (child) => {
            if (React.isValidElement(child)) {
              return React.cloneElement(child, { id, variant } as any)
            }
            return child
          })}
        </div>
        <ShadcnCheckbox
          id={id}
          checked={isChecked}
          onCheckedChange={handleCheckedChange}
          className={cn(
            'transition-colors pointer-events-none flex-shrink-0',
            variant === 'primary' && 'text-primary border-primary'
          )}
        />
      </div>
    )
  }
)
Checkbox.displayName = 'Checkbox'

const labelVariants = cva(
  'text-sm font-medium leading-none cursor-pointer peer-disabled:cursor-not-allowed peer-disabled:opacity-70',
  {
    variants: {
      variant: {
        default: 'text-foreground',
        outline: 'text-foreground',
        primary: 'text-primary'
      }
    },
    defaultVariants: {
      variant: 'outline'
    }
  }
)

interface LabelProps
  extends React.LabelHTMLAttributes<HTMLLabelElement>,
    VariantProps<typeof labelVariants> {
  id?: string
}

const CheckboxLabel = React.forwardRef<HTMLLabelElement, LabelProps>(
  ({ id, variant, className, ...props }, ref) => (
    <label ref={ref} className={cn(labelVariants({ variant, className }))} {...props} />
  )
)
CheckboxLabel.displayName = 'Label'

const descriptionVariants = cva('text-sm', {
  variants: {
    variant: {
      default: 'text-muted-foreground',
      outline: 'text-muted-foreground',
      primary: 'text-primary/80'
    }
  },
  defaultVariants: {
    variant: 'outline'
  }
})

interface DescriptionProps
  extends React.HTMLAttributes<HTMLParagraphElement>,
    VariantProps<typeof descriptionVariants> {}

const CheckboxDescription = React.forwardRef<HTMLParagraphElement, DescriptionProps>(
  ({ variant, className, ...props }, ref) => (
    <div ref={ref} className={cn(descriptionVariants({ variant, className }))} {...props} />
  )
)
CheckboxDescription.displayName = 'Description'

export { CheckboxDescription, CheckboxLabel, Checkbox as SwitchButton }
