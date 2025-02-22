import { Button } from '@renderer/components/ui/button'
import { Textarea } from '@renderer/components/ui/textarea'
import { QTooltip } from '@renderer/components/ui/tooltip'
import { trpc } from '@renderer/lib/trpc-link'
import { cn } from '@renderer/lib/utils'
import { logger } from '@shared/logger'
import { LucideFlame } from 'lucide-react'
import { useMemo } from 'react'
import { toast } from 'sonner'
import { useLinkStore } from './add-link.store'
import { useApp } from './app-context'
import SelectDownloadBox from './select-download-path'
const httpsRegex = /^https?/i
export default function AddLink({ showDownloadPath }: { showDownloadPath?: boolean }) {
  const { settings, setSetting } = useApp()
  const { mutateAsync: queueDownloadFromUrl, isLoading } = trpc.ytdl.downloadMedia.useMutation({
    onError(error, variables, context) {
      toast.error(error.data!.code, { description: error.message })
    }
  })
  const [mediaUrl, setMediaUrl] = useLinkStore()
  const linkCount = useMemo(
    () => mediaUrl.split('\n').filter((url) => url && httpsRegex.test(url)).length,
    [mediaUrl]
  )

  logger.debug('add-link', { mediaUrl })
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-0.5">
        <Textarea
          placeholder="https://youtube.com/watch?v=xyz"
          className="placeholder:text-xs text-[0.775rem]"
          value={mediaUrl}
          onChange={(ev) => setMediaUrl(ev.target.value)}
          rows={5}
        />
        <div className="flex items-end justify-end text-xs text-muted-foreground mr-2">
          {linkCount} links captured
        </div>
      </div>
      <div className="flex items-center gap-2">
        <SelectDownloadBox></SelectDownloadBox>
        <div className="flex-auto"></div>
        <QTooltip content="Enable/Disable clipboard monitoring">
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
        </QTooltip>

        <Button
          disabled={isLoading || !mediaUrl}
          onClick={() => {
            if (!mediaUrl) return
            const queueUrls = [...mediaUrl.split('\n').filter((s) => {
              logger.debug({regexTestSource: s})
              return s && httpsRegex.test(s)
            })]
            logger.debug('download requested for ', { url: queueUrls, mediaUrl })
            queueDownloadFromUrl({ url: queueUrls })
            setMediaUrl('')
          }}
        >
          Add & Start Download
        </Button>
      </div>
    </div>
  )
}
