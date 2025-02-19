import ClickableText from '@renderer/components/ui/clickable-text'
import { useIPC } from '@renderer/hooks/use-ipc'
import { trpc } from '@renderer/lib/trpc-link'
import { DotIcon } from 'lucide-react'
import prettyBytes from 'pretty-bytes'
import { useMemo } from 'react'
import { toast } from 'sonner'
import { useApp } from './app-context'
import { useYtdl } from './ytdl-context'

export default function StatusBar() {
  const { settings } = useApp()
  const { status } = useYtdl()
  const [data] = trpc.ytdl.stats.useSuspenseQuery()
  const { mutateAsync: quitAndUpdate } = trpc.internals.quitAndInstallUpdate.useMutation({
    onError(error, variables, context) {
      toast.error(error.message)
    }
  })
  const diskUsage = useMemo(() => data && prettyBytes(data.completedUsage), [data])
  const [updateAvailable] = useIPC('update-available')
  const [updateChecking] = useIPC('update-checking')
  const [updateProgress] = useIPC('update-download-progress')
  const [updateDone] = useIPC('update-download-done')
  const latestYtdlpStatus = useMemo(() => status[0], [status])
  return (
    <div className="grid items-center grid-cols-[200px_1fr_200px] h-10 flex-shrink-0 overflow-hidden border-t border-t-border flex-auto text-xs text-muted-foreground px-4 select-none">
      <div className="flex gap-1 items-center">
        <span>Disk usage:</span>
        <span>{diskUsage}</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="flex gap-1 items-center">
          <span>Total:</span>
          <span>{data.overallCount}</span>
        </div>
        <DotIcon className="size-2 text-muted-foreground" />
        <div className="flex gap-1 items-center">
          <span>Videos:</span>
          <span>{data.count.video}</span>
        </div>
        <DotIcon className="size-2 text-muted-foreground" />
        <div className="flex gap-1 items-center">
          <span>Audios:</span>
          <span>{data.count.audio}</span>
        </div>
      </div>
      <div className="flex items-center gap-2 justify-end">
        {updateChecking && !updateAvailable ? (
          <div className="animate-pulse">Checking for updates...</div>
        ) : updateDone ? (
          <ClickableText onClick={() => quitAndUpdate()}>
            Restart required for update.
          </ClickableText>
        ) : updateProgress && !updateDone ? (
          <div>Downloading... {String(updateProgress.percent).padStart(3, ' ')}%</div>
        ) : updateAvailable ? (
          <div>Update Available, preparing...</div>
        ) : !latestYtdlpStatus ? (
          <div>YTDLP: {settings.ytdlp?.version ?? '...'}</div>
        ) : (
          <div>YTDLP: {latestYtdlpStatus.state}</div>
        )}
      </div>
    </div>
  )
}
