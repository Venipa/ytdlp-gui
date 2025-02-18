import { Button } from '@renderer/components/ui/button'
import { Input } from '@renderer/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@renderer/components/ui/tooltip'
import { trpc } from '@renderer/lib/trpc-link'
import { cn } from '@renderer/lib/utils'
import { LucideFlame } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { useApp } from './app-context'

export default function AddLink() {
  const { settings, setSetting } = useApp()
  const { mutateAsync: addMediaUrl, isLoading } = trpc.ytdl.downloadMedia.useMutation({
    onError(error, variables, context) {
      toast.error(error.data!.code, { description: error.message })
    }
  })
  const [mediaUrl, setMediaUrl] = useState('')
  return (
    <div className="flex items-center gap-2">
      <Tooltip delayDuration={500}>
        <TooltipTrigger>
          <Button
            disabled={isLoading}
            variant={'ghost'}
            onClick={() =>
              setSetting('features.clipboardMonitor', !settings.features.clipboardMonitor)
            }
          >
            <LucideFlame
              className={cn(
                settings.features.clipboardMonitor
                  ? 'fill-yellow-300 stroke-yellow-600'
                  : 'stroke-primary',
                'transition-colors duration-200 ease-out'
              )}
            />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Enable/Disable clipboard monitoring</TooltipContent>
      </Tooltip>
      <Input
        placeholder="https://youtube.com/watch?v=xyz"
        className="placeholder:text-xs text-[0.775rem]"
        defaultValue={mediaUrl}
        onChange={(ev) => setMediaUrl(ev.target.value)}
      />
      <Button
        disabled={isLoading || !mediaUrl}
        onClick={() => {
          addMediaUrl({ url: mediaUrl })
        }}
      >
        Add
      </Button>
    </div>
  )
}
