import { trpc } from '@renderer/lib/trpc-link'
import { logger } from '@shared/logger'
import { LucideCog } from 'lucide-react'
import SettingsToggle from '../components/settings-toggle'

export const meta = {
  title: 'Settings',
  icon: LucideCog,
  index: 10,
  show: true
}
const Icon = meta.icon
export default function SettingsTab() {
  const { mutateAsync: checkUpdate } = trpc.internals.checkUpdate.useMutation()
  return (
    <div className="grid gap-8 p-2 h-full">
      <div className="grid gap-6 pt-6">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Icon className="size-5" />
            <h1 className="text-lg font-semibold">Settings</h1>
          </div>
        </div>
        <div className="flex flex-col gap-6">
          <SettingsToggle
            name="beta"
            onChange={() => checkUpdate().then(logger.debug.bind(logger))}
          >
            <span>Sign up for Beta releases.</span>
          </SettingsToggle>
          <div className="grid grid-cols-[18px_1fr] gap-2 group/parent">
            <div className="grid grid-rows-[18px_1fr] gap-2 justify-items-center">
              <div className="size-1.5 bg-muted-foreground rounded-full self-end transition-colors duration-200 ease-out group-hover/parent:bg-primary"></div>
              <div className="w-px bg-muted transition-colors ease-in-out duration-200 origin-center group-hover/parent:bg-primary"></div>
            </div>
            <div className="grid grid-rows-[40px_1fr]">
              <h1 className="text-lg font-bold tracking-wide">Clipboard Monitor</h1>
              <div className="flex flex-col gap-2">
                <SettingsToggle name="features.clipboardMonitor">
                  <div className="flex flex-col gap-2">
                    <span>Enable Clipboard Monitor</span>
                    <span className="text-muted-foreground">
                      Automatically adds any link to the request form.
                    </span>
                  </div>
                </SettingsToggle>
                <SettingsToggle name="features.clipboardMonitorAutoAdd">
                  <div className="flex flex-col gap-2">
                    <span>Automatically add links to queue</span>

                    <span className="text-muted-foreground">
                      Activating this will Automatically start the download in the request form.
                    </span>
                  </div>
                </SettingsToggle>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
