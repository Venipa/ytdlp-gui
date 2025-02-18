import { CheckboxButton } from '@renderer/components/ui/checkbox-button'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { trpc } from '@renderer/lib/trpc-link'
import { logger } from '@shared/logger'
import { LucideCog } from 'lucide-react'
import { useApp } from '../components/app-context'

export const meta = {
  title: 'Settings',
  icon: LucideCog,
  index: 10,
  show: true
}
const Icon = meta.icon
export default function SettingsTab() {
  const { settings, setSetting } = useApp()
  const { mutateAsync: checkUpdate } = trpc.internals.checkUpdate.useMutation()
  return (
    <div className="grid gap-8 p-2">
      <div className="grid gap-12">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Icon className="size-5" />
            <h1 className="text-lg font-semibold">Settings</h1>
          </div>
        </div>
        <ScrollArea className="flex flex-col gap-6">
          <CheckboxButton
            className="flex items-center text-sm"
            defaultChecked={settings.beta}
            onCheckedChange={() =>
              setSetting('beta', !settings.beta)
                .then(() => checkUpdate())
                .then(logger.debug.bind(logger))
            }
          >
            <span>Sign up for Beta releases.</span>
          </CheckboxButton>
        </ScrollArea>
      </div>
    </div>
  )
}
