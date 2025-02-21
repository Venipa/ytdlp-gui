import { Input } from '@renderer/components/ui/input'
import { Label } from '@renderer/components/ui/label'
import { get } from 'lodash'
import { ReactNode, useEffect, useMemo } from 'react'
import { useDebounceValue } from 'usehooks-ts'
import { useApp } from './app-context'

type SettingsToggleProps = { name: string; onChange?: (value: string) => void; title: string | ReactNode }
export default function SettingsInput({
  name: key,
  title: placeholder,
  ...props
}: SettingsToggleProps) {
  const { settings, setSetting } = useApp()
  const settingsValue = useMemo(() => get(settings, key), [key, settings])
  const [debouncedValue, setDebouncedValue] = useDebounceValue<string>(settingsValue, 1000)
  useEffect(() => {
    if (settingsValue !== debouncedValue) setSetting(key, debouncedValue)
  }, [debouncedValue])
  return (
    <div className="flex flex-col gap-1">
      <Label>{placeholder}</Label>
      <Input defaultValue={debouncedValue} onChange={(ev) => setDebouncedValue(ev.target.value)} />
    </div>
  )
}
