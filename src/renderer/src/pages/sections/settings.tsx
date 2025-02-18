import { CheckboxButton } from '@renderer/components/ui/checkbox-button'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { LucideCog } from 'lucide-react'

export const meta = {
  title: 'Settings',
  icon: LucideCog,
  index: 10,
  show: false
}
const Icon = meta.icon
export default function SettingsTab() {
  return (
    <div className="grid gap-8 p-2">
      <div className="grid gap-12">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Icon className='size-5' />
            <h1 className="text-lg font-semibold">Settings</h1>
          </div>
        </div>
        <ScrollArea className="flex flex-col gap-6">
          <CheckboxButton className="flex items-center text-sm">
            <span>Sign up for Beta releases.</span>
          </CheckboxButton>
        </ScrollArea>
      </div>
    </div>
  )
}
