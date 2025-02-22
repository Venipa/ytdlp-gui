import { Input, InputProps } from '@renderer/components/ui/input'
import { Label } from '@renderer/components/ui/label'
import { cn } from '@renderer/lib/utils'
import { cva, VariantProps } from 'class-variance-authority'
import { clamp, get } from 'lodash'
import { forwardRef, useEffect, useMemo, useState } from 'react'
import { useDebounceValue } from 'usehooks-ts'
import { useApp } from './app-context'
const inputVariants = cva('', {
  variants: {
    variant: {
      vertical: 'flex flex-col gap-1',
      horizontal: 'flex flex-row gap-0 items-center h-10'
    }
  },
  defaultVariants: {
    variant: 'vertical'
  }
})
const inputFieldVariants = cva('', {
  variants: {
    variant: {
      vertical: '',
      horizontal: 'rounded-l-none h-full'
    }
  },
  defaultVariants: {
    variant: 'vertical'
  }
})
const inputLabelVariants = cva('relative', {
  variants: {
    variant: {
      vertical: '',
      horizontal:
        'bg-muted/40 h-full border border-r-0 border-border rounded-s-md flex justify-center items-center px-2.5 text-right'
    }
  },
  defaultVariants: {
    variant: 'vertical'
  }
})
type SettingsToggleProps = {
  name: string
  onChange?: (value: any) => void
  title: any
  hint?: any
} & InputProps &
  VariantProps<typeof inputVariants>
export default forwardRef<HTMLInputElement, SettingsToggleProps>(function SettingsInput(
  { className, name: key, title: placeholder, hint, ...props },
  ref
) {
  const { settings, setSetting } = useApp()
  const settingsValue = useMemo(() => get(settings, key), [key, settings])
  const [debouncedValue, setDebouncedValue] = useDebounceValue<any>(settingsValue, 1000)
  const [value, setValue] = useState(() => settingsValue)
  useEffect(() => {
    if (settingsValue !== debouncedValue)
      setSetting(key, debouncedValue).then(() => {
        setValue(debouncedValue)
      })
  }, [debouncedValue])
  return (
    <div className={cn("flex flex-col gap-1", hint && "pb-4")}>
      <div className={cn(inputVariants(props))}>
        <Label className={cn(inputLabelVariants(props))}>{placeholder}</Label>
        {/* {props.variant === 'horizontal' && <div className="w-px bg-muted h-full"></div>} */}
        <Input
          ref={ref}
          defaultValue={debouncedValue}
          onChange={(ev) => {
            setValue(ev.target.value)
            if (ev.target.type === 'number')
              return setDebouncedValue(
                clamp(
                  ev.target.valueAsNumber,
                  (ev.target.min && Number(ev.target.min)) || 0,
                  (ev.target.max && Number(ev.target.max)) || ev.target.valueAsNumber
                )
              )
            return setDebouncedValue(ev.target.value)
          }}
          value={value}
          className={cn(inputFieldVariants(props))}
          {...props}
        />
      </div>
      {typeof hint === "string" && <div className="text-muted-foreground text-xs flex justify-end">{hint}</div> || hint}
    </div>
  )
})
