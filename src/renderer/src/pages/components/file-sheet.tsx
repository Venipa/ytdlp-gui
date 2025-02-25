import type { SelectDownload } from '@main/stores/queue-database.helpers'
import { Button } from '@renderer/components/ui/button'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from '@renderer/components/ui/sheet'
import { trpc } from '@renderer/lib/trpc-link'
import { formatDistanceToNow } from 'date-fns'
import prettyBytes from 'pretty-bytes'
import { PropsWithChildren, useMemo } from 'react'
const isYTDomain = /^((www|music)\.)?youtube.com/
export default function FileSheet({
  item: { title, source, url, metaId, filepath, filesize, meta, type, created },
  children
}: PropsWithChildren<{ item: SelectDownload }>) {
  const { isYoutube, frameSource } = useMemo(() => {
    const isYoutube = isYTDomain.test(source)
    const frameSource =
      (isYoutube &&
        metaId &&
        `https://www.youtube.com/embed/${metaId}?rel=0&showinfo=0&controls=1&iv_load_policy=3&playsinline=1&fs=0`) ||
      null
    return { isYoutube, frameSource }
  }, [source])
  const { mutateAsync: openFile } = trpc.internals.openFile.useMutation()
  const createdAt = useMemo(
    () => (created && formatDistanceToNow(new Date(created), { includeSeconds: false })) || null,
    [created]
  )
  return (
    <>
      <Sheet modal>
        <SheetTrigger asChild>{children}</SheetTrigger>
        <SheetContent className="flex flex-col gap-6 sm:max-w-2xl h-full">
          <SheetTitle className="text-lg font-semibold tracking-wider whitespace-pre-wrap mr-8 break-words line-clamp-2 flex-shrink-0">
            {title}
          </SheetTitle>
          {isYoutube && frameSource && (
            <div className="-mx-6 flex-shrink-0">
              <iframe
                className="w-full aspect-video"
                src={frameSource}
                title="YouTube video player"
                allow="encrypted-media;"
                referrerpolicy="strict-origin-when-cross-origin"
              ></iframe>
            </div>
          )}
          <ScrollArea className="flex-auto flex flex-col -m-6 h-full" plain>
            <div className="file-info-row">
              <span>Filename</span>
              <span>{meta?.filename || '-'}</span>
            </div>
            <div className="file-info-row">
              <span>Filesize</span>
              <span>{filesize && prettyBytes(filesize) || '0B'}</span>
            </div>
            <div className="file-info-row">
              <span>Type</span>
              <span>{type || '-'}</span>
            </div>
            <div className="file-info-row">
              <span>Source</span>
              <span>{source || '-'}</span>
            </div>
            <div className="file-info-row">
              <span>Uploader</span>
              <div className="flex items-center gap-2">
                {meta?.uploader && meta?.uploader !== meta?.channel && (
                  <>
                    <span>{meta?.uploader || '-'}</span>
                    <span className='bg-muted-foreground size-1 rounded-full'></span>
                  </>
                )}
                <span>{meta?.channel || '-'}</span>
              </div>
            </div>
            <div className="file-info-row">
              <span></span>
              <span>downloaded {createdAt || '?'} ago</span>
            </div>
          </ScrollArea>
          <div className="flex items-center gap-3 -mx-6 px-6 pt-6 border-t border-t-muted justify-end flex-shrink-0">
            <Button asChild variant={'outline'}>
              <a href={url} target="_blank">
                Open in Browser
              </a>
            </Button>
            <Button variant={'outline'} onClick={() => openFile({ path: filepath })}>
              Open File
            </Button>
          </div>
        </SheetContent>
      </Sheet>
      <style jsx>
        {`
          .file-info-row {
            @apply h-10 grid grid-cols-[160px_1fr] gap-6 items-center text-sm flex-shrink-0;
          }
          .file-info-row:nth-child(even) {
            @apply bg-muted/20;
          }
          .file-info-row > span:first-of-type {
            @apply flex justify-end;
          }
          .file-info-row > span:nth-child(2) {
            @apply truncate pr-8;
          }

        `}
      </style>
    </>
  )
}
