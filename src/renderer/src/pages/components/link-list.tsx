import { Button } from '@renderer/components/ui/button'
import ClickableText from '@renderer/components/ui/clickable-text'
import { ProgressCircle } from '@renderer/components/ui/progress-circle'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { Spinner } from '@renderer/components/ui/spinner'
import SuspenseLoader from '@renderer/components/ui/suspense-loader'
import { QTooltip, Tooltip, TooltipContent, TooltipTrigger } from '@renderer/components/ui/tooltip'
import { trpc } from '@renderer/lib/trpc-link'
import { createLogger, logger } from '@shared/logger'
import {
  DotIcon,
  LucideArrowDownToDot,
  LucideCheck,
  LucideFolderOpen,
  LucidePause,
  LucideRedo2,
  LucideSave,
  LucideSquare,
  LucideTrash,
  LucideX
} from 'lucide-react'
import prettyBytes from 'pretty-bytes'
import { useMemo, useState } from 'react'
import { YTDLDownloadStatus, YTDLItem } from 'ytdlp-desktop/types'
const log = createLogger('LinkListItem')
export function LinkListItem({
  id,
  error: ytderror,
  state,
  filesize: fsize,
  type,
  source,
  title,
  filepath,
  url
}: YTDLItem) {
  const error = useMemo(() => ytderror, [ytderror])
  const [status, setDownloadStatus] = useState<YTDLDownloadStatus>()
  const { mutateAsync: openPath } = trpc.internals.openPath.useMutation()
  const filesize = useMemo(() => prettyBytes(fsize), [fsize])
  trpc.ytdl.onIdDownload.useSubscription(id, {
    onData(data) {
      if (data) setDownloadStatus(data as any)

      log.debug('onIdDownload', data)
    }
  })
  const completed = useMemo(() => state === 'completed', [state, status])
  const cancelled = useMemo(() => state === 'cancelled', [state, status])
  const downloading = useMemo(() => state === 'downloading', [state, status])
  return (
    <div className="h-16 rounded-md hover:bg-muted/20 grid grid-cols-[40px_1fr_minmax(100px,_auto)] gap-2 items-center relative cursor-default group/item flex-shrink-0 select-none">
      <div className="flex flex-col size-10 items-center justify-center">
        {error ? (
          <QTooltip content={'An error occurred while downloading.'}>
            <div className="size-5 p-1 flex flex-col items-center justify-center border-2 border-destructive/40 bg-destructive rounded-full">
              <LucideX className="stroke-[4px] stroke-destructive-foreground" />
            </div>
          </QTooltip>
        ) : cancelled ? (
          <div className="size-5 p-1 flex flex-col items-center justify-center border-2 border-muted rounded-full">
            <LucideSquare className="stroke-none fill-current" />
          </div>
        ) : completed ? (
          <div className="size-5 p-1 flex flex-col items-center justify-center bg-green-500 text-white rounded-full">
            <LucideCheck className="stroke-[4px]" />
          </div>
        ) : downloading && status ? (
          <Tooltip delayDuration={500}>
            <TooltipTrigger className="cursor-default">
              <div className="flex flex-col items-center justify-center size-10 relative">
                <ProgressCircle
                  min={0}
                  max={100}
                  value={status.percent ?? 0}
                  className="h-6"
                  gaugePrimaryColor="rgb(225 225 225)"
                  gaugeSecondaryColor="rgba(120, 120, 120, 0.1)"
                  showValue={false}
                />
                <LucideArrowDownToDot className="absolute size-3.5 text-secondary-foreground animate-pulse" />
              </div>
            </TooltipTrigger>
            <TooltipContent>Download Progress</TooltipContent>
          </Tooltip>
        ) : (
          <Spinner />
        )}
      </div>
      <div className="grid grid-rows-[20px_12px_16px] items-center">
        <div className="text-sm truncate">{title}</div>
        <div className="flex gap-2.5 items-center text-xs text-muted-foreground">
          {completed && (
            <>
              <span>Filesize: {filesize}</span>
              <DotIcon className="size-2 -mx-2" />
            </>
          )}
          <span>Type: {type}</span>
          <DotIcon className="size-2 -mx-2" />
          {!url ? (
            <span>Source: {source}</span>
          ) : (
            <ClickableText asChild>
              <a className="cursor-pointer" href={url} target="_blank">
                Source: {source}
              </a>
            </ClickableText>
          )}
        </div>
        <div className="flex gap-2.5 items-center text-xs text-muted-foreground truncate">
          {completed && (
            <>
              <div className="flex items-center gap-1">
                <LucideSave className="size-3 flex-shrink-0" />
                <span className="truncate group-hover/item:max-w-fit max-w-[200px]">{filepath}</span>
              </div>
            </>
          )}
        </div>
      </div>
      <div className="flex justify-end items-center gap-2 px-2 opacity-20 group-hover/item:opacity-100">
        {(error || cancelled) && (
          <Button variant={'ghost'} size={'icon'} className="px-2.5">
            <LucideRedo2 className="stroke-[3px]" />
          </Button>
        )}
        {downloading && (
          <Button variant={'ghost'} size={'icon'} className="px-2.5">
            <LucidePause className="fill-current stroke-none" />
          </Button>
        )}
        {completed && filepath && (
          <Button
            variant={'ghost'}
            size={'icon'}
            className="px-2.5"
            onClick={() => openPath(filepath)}
          >
            <LucideFolderOpen className="fill-current stroke-none" />
          </Button>
        )}
        {completed && (
          <Button
            variant={'ghost'}
            size={'icon'}
            className="px-2.5 text-red-500 hover:text-red-400"
          >
            <LucideTrash className="fill-current stroke-none" />
          </Button>
        )}
        {!completed && (
          <Button
            variant={'ghost'}
            size={'icon'}
            className="px-2.5 text-red-500 hover:text-red-400 opacity-0 group-hover/item:opacity-100"
          >
            <LucideX className="fill-current stroke-none" />
          </Button>
        )}
      </div>
    </div>
  )
}
export default function LinkList() {
  const {
    data: items,
    isFetching,
    refetch
  } = trpc.ytdl.list.useQuery(undefined, {
    initialData: [] as YTDLItem[]
  })
  const {
    ytdl: { list }
  } = trpc.useUtils()
  trpc.ytdl.listSync.useSubscription(undefined, {
    onData(data: YTDLItem[]) {
      if (data?.length) {
        list.setData(undefined, (state) => {
          if (!state) state = []
          data.forEach((item) => {
            const idx = state.findIndex((d) => d.id === item.id)
            if (idx !== -1) state.splice(idx, 1, item)
            else [item, ...state]
          })
          logger.debug('listSync', { state })
          return [...state]
        })
        list.invalidate()
      }
    }
  })
  logger.debug({ items })
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 mx-2">
        <h1 className="text-xs text-muted-foreground">Download list</h1>
      </div>
      <ScrollArea className="h-[300px] border border-muted rounded-lg relative">
        {isFetching && <SuspenseLoader className="absolute bg-background inset-0" />}
        <div className="flex flex-col gap-2 py-2.5 px-2 h-full">
          {items.map((d) => (
            <LinkListItem key={d.id} {...d} />
          ))}
        </div>
      </ScrollArea>
    </div>
  )
}
