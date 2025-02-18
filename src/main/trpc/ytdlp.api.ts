import { TRPCError } from '@trpc/server'
import { observable } from '@trpc/server/observable'
import filenamify from 'filenamify'
import { statSync } from 'fs'
import path from 'path'
import { VideoInfo } from 'yt-dlp-wrap/types'
import { YTDLDownloadStatus, YTDLItem, YTDLStatus } from 'ytdlp-desktop/types'
import { z } from 'zod'
import { publicProcedure, router } from './trpc'
import { ytdl } from './ytdlp.core'
import { ytdlpEvents } from './ytdlp.ee'
const ytdlpDownloads = new Map<string, YTDLItem>()
export const ytdlpRouter = router({
  state: publicProcedure.query(() => ytdl.state.toString()),
  checkUpdates: publicProcedure.mutation(() => ytdl.checkUpdates()),
  downloadMedia: publicProcedure
    .input(
      z.object({
        url: z.string().url()
      })
    )
    .mutation(async ({ input: { url }, ctx }) => {
      return await handleYtdlMedia(url)
    }),
  status: publicProcedure.subscription(() => {
    return observable<YTDLStatus>((emit) => {
      function onStatusChange(data: any) {
        emit.next(data as YTDLStatus)
      }

      ytdlpEvents.on('status', onStatusChange)

      return () => {
        ytdlpEvents.off('status', onStatusChange)
      }
    })
  }),
  onDownload: publicProcedure.subscription(() => {
    return observable<YTDLDownloadStatus>((emit) => {
      function onStatusChange(data: any) {
        emit.next(data)
      }

      ytdlpEvents.on('download', onStatusChange)

      return () => {
        ytdlpEvents.off('download', onStatusChange)
      }
    })
  }),
  onIdDownload: publicProcedure.input(z.string()).subscription(({ input: id }) => {
    return observable<YTDLDownloadStatus>((emit) => {
      function onStatusChange(data: { id: string }) {
        if (data.id === id) emit.next(data as any)
      }

      ytdlpEvents.on('download', onStatusChange)

      return () => {
        ytdlpEvents.off('download', onStatusChange)
      }
    })
  }),
  list: publicProcedure.query(async () => Array.from(ytdlpDownloads.values())),
  listSync: publicProcedure.subscription(() => {
    return observable<YTDLItem[]>((emit) => {
      function onStatusChange(data: any) {
        emit.next(data)
      }

      ytdlpEvents.on('list', onStatusChange)

      return () => {
        ytdlpEvents.off('list', onStatusChange)
      }
    })
  })
} as const)
const handleYtdlMedia = async (url: string) => {
  if (typeof url !== 'string' || !/^https/gi.test(url)) return

  ytdlpEvents.emit('status', { action: 'getVideoInfo', state: 'progressing' })
  const videoInfo: VideoInfo = await ytdl.ytdlp.getVideoInfo(url)
  if (!videoInfo) throw new TRPCError({ code: 'NOT_FOUND', message: 'Video not found' })
  ytdlpEvents.emit('status', { action: 'getVideoInfo', state: 'done' })
  const filepath = path.join(
    ytdl.currentDownloadPath,
    `${filenamify(videoInfo.title)}${videoInfo.id ? `_(${videoInfo.id}` : ''}.mp4`
  )
  const stream = ytdl.ytdlp.exec([url, '-f', 'best[ext=mp4]', '-o', filepath, '--no-mtime'])
  ytdlpDownloads.set(videoInfo.id, {
    id: videoInfo.id,
    filepath,
    filesize: 0,
    source: new URL(url).hostname,
    state: 'downloading',
    title: videoInfo.title,
    type: 'video',
    url: url,
    error: null,
    retryCount: 0
  })
  const dbFile = ytdlpDownloads.get(videoInfo.id)!
  ytdlpEvents.emit('list', [dbFile])
  stream.on('progress', (ev) => {
    ytdlpEvents.emit('status', {
      id: videoInfo.id,
      action: 'download',
      data: ev,
      state: 'progressing'
    })
    ytdlpEvents.emit('download', { ...ev, id: videoInfo.id })
  })
  stream.once('progress', () => {
    ytdlpEvents.emit('list', [dbFile])
  })
  stream.once('close', () => {
    ytdlpEvents.emit('status', {
      id: videoInfo.id,
      action: 'download',
      data: videoInfo,
      state: 'done'
    })
    ytdlpEvents.emit('download', { id: videoInfo.id, percent: 100 })
  })
  stream.on('error', (error) => {
    ytdlpEvents.emit('status', { id: videoInfo.id, action: 'download', error, state: 'error' })
    ytdlpEvents.emit('download', { id: videoInfo.id, percent: 100, error })
    dbFile.state = 'error'
    dbFile.error = error
  })
  await new Promise((resolve) => stream.once('close', resolve))
  if (!dbFile.error) {
    const fileStats = statSync(filepath)
    dbFile.filesize = fileStats.size
    dbFile.state = 'completed'
  }
  ytdlpEvents.emit('list', [dbFile])
  return dbFile
}
ytdlpEvents.on('add', handleYtdlMedia)
process.on('exit', () => {
  ytdlpEvents.off('add', handleYtdlMedia)
})
