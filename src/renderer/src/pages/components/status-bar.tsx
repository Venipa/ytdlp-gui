import ClickableText from '@renderer/components/ui/clickable-text'
import { Appear } from '@renderer/components/ui/routes/animated-content'
import { Spinner } from '@renderer/components/ui/spinner'
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
  const [updateAvailable, setUpdateAvailable] = useIPC('update-available')
  const [updateChecking, setUpdateChecking] = useIPC('update-checking')
  const [updateProgress, setUpdateProgress] = useIPC('update-download-progress')
  const [updateDone, setUpdateDone] = useIPC('update-download-done')
  const latestYtdlpStatus = useMemo(() => status[0], [status])
  const noStateOrInvalidStatus = useMemo(
    () => !latestYtdlpStatus || ['done', 'deleted'].includes(latestYtdlpStatus.state),
    [latestYtdlpStatus]
  )
  return (
    <div className="grid items-center grid-cols-[140px_200px_1fr] h-10 flex-shrink-0 overflow-hidden border-t border-t-border flex-auto text-xs text-muted-foreground px-4 select-none">
      <Appear className="flex gap-1 items-center">
        <span>Disk usage:</span>
        <span>{diskUsage}</span>
      </Appear>
      <div className="flex items-center gap-2">
        <Appear className="flex gap-1 items-center">
          <span>Total:</span>
          <span>{data.overallCount}</span>
        </Appear>
        <DotIcon className="size-2 text-muted-foreground" />
        <Appear className="flex gap-1 items-center">
          <span>Videos:</span>
          <span>{data.count.video}</span>
        </Appear>
        <DotIcon className="size-2 text-muted-foreground" />
        <Appear className="flex gap-1 items-center">
          <span>Audios:</span>
          <span>{data.count.audio}</span>
        </Appear>
      </div>
      <div className="flex items-center gap-4 justify-end">
        {updateChecking && !updateAvailable ? (
          <>
            <Appear className="animate-pulse">Checking for updates...</Appear>
            <div className="h-10 w-px bg-muted"></div>
          </>
        ) : updateDone ? (
          <>
            <ClickableText onClick={() => quitAndUpdate()}>
              Click to restart to install update.
            </ClickableText>
            <div className="h-10 w-px bg-muted"></div>
          </>
        ) : updateProgress && !updateDone ? (
          <>
            <Appear>Downloading... {String(updateProgress.percent).padStart(3, ' ')}%</Appear>
            <div className="h-10 w-px bg-muted"></div>
          </>
        ) : updateAvailable ? (
          <>
            <Appear>Update Available, preparing...</Appear>
            <div className="h-10 w-px bg-muted"></div>
          </>
        ) : null}
        {noStateOrInvalidStatus ? (
          <Appear>YTDLP: {settings.ytdlp?.version ?? '...'}</Appear>
        ) : latestYtdlpStatus.state === 'progressing' ? (
          <Appear className="flex items-center gap-1">
            <span>YTDLP:</span>
            <Spinner size={'xs'} />
          </Appear>
        ) : (
          <Appear>YTDLP: {latestYtdlpStatus.state}</Appear>
        )}
      </div>
    </div>
  )
}
