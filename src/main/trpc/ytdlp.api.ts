import { db } from '@main/stores/queue-database'
import { downloads } from '@main/stores/queue-database.schema'
import { TRPCError } from '@trpc/server'
import { observable } from '@trpc/server/observable'
import { desc, eq } from 'drizzle-orm'
import filenamify from 'filenamify'
import { statSync } from 'fs'
import { omit } from 'lodash'
import path from 'path'
import type { VideoInfo } from 'yt-dlp-wrap/types'
import { YTDLDownloadStatus, YTDLItem, YTDLStatus } from 'ytdlp-desktop/types'
import { z } from 'zod'
import { publicProcedure, router } from './trpc'
import { ytdl } from './ytdlp.core'
import { ytdlpEvents } from './ytdlp.ee'
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
  cancel: publicProcedure
    .input(z.union([z.string(), z.number()]))
    .mutation(({ input: id }) => ytdlpEvents.emit('cancel', id)),
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
  retry: publicProcedure.input(z.number()).mutation(async ({ input: id }) => {
    const dbFile = await db.query.downloads.findFirst({
      where(fields, { eq }) {
        return eq(fields.id, id)
      }
    })
    if (!dbFile) throw new TRPCError({ code: 'NOT_FOUND', message: 'id not found in database' })
    ytdlpEvents.emit('add', dbFile.url)
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
  onIdDownload: publicProcedure.input(z.number()).subscription(({ input: id }) => {
    return observable<YTDLDownloadStatus>((emit) => {
      function onStatusChange(data: { id: number }) {
        if (data.id === id) emit.next(data as any)
      }

      ytdlpEvents.on('download', onStatusChange)

      return () => {
        ytdlpEvents.off('download', onStatusChange)
      }
    })
  }),
  stats: publicProcedure.query(async () => {
    const items = await db.select().from(downloads).all()
    return items.reduce(
      (acc, r) => {
        acc.overallCount++
        const type = r.type?.toLowerCase()
        if (type) acc.count[type]++
        if (r.state) acc.state[r.state.toLowerCase()]++
        if (r.filesize) {
          acc.overallUsage += r.filesize
          if (type) {
            acc.size[type] += r.filesize
          }
          if (r.state === 'completed') acc.completedUsage += r.filesize
        }
        return acc
      },
      {
        overallUsage: 0,
        completedUsage: 0,
        overallCount: 0,
        state: { completed: 0, downloading: 0, error: 0, cancelled: 0 },
        count: { video: 0, audio: 0, other: 0 },
        size: { video: 0, audio: 0, other: 0 }
      }
    )
  }),
  list: publicProcedure.query(
    async () => await db.select().from(downloads).orderBy(desc(downloads.created)).all()
  ),
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
  let [dbFile] = await db
    .insert(downloads)
    .values({
      metaId: '',
      meta: {} as any,
      filepath: '',
      filesize: 0,
      source: new URL(url).hostname,
      state: 'fetching_meta',
      title: url,
      type: 'video',
      url,
      error: null,
      retryCount: 0
    })
    .returning()
  ytdlpEvents.emit('list', [dbFile])
  const videoInfo: VideoInfo = await ytdl.ytdlp.getVideoInfo(url)
  if (!videoInfo) throw new TRPCError({ code: 'NOT_FOUND', message: 'Video not found' })
  ytdlpEvents.emit('status', { action: 'getVideoInfo', state: 'done' })
  const filepath = path.join(
    ytdl.currentDownloadPath,
    `${filenamify(videoInfo.title)}${videoInfo.id ? `_(${videoInfo.id})` : ''}.mp4`
  )
  dbFile.meta = omit(videoInfo, ['formats']) as any
  dbFile.metaId = videoInfo.id
  dbFile.filepath = filepath
  dbFile.title = videoInfo.title
  dbFile.state = 'downloading'
  dbFile = await db
    .update(downloads)
    .set(omit(dbFile, 'id'))
    .where(eq(downloads.id, dbFile.id))
    .returning()
    .then(([newDbFile]) => newDbFile)
  const controller = new AbortController()

  const stream = ytdl.ytdlp.exec(
    [url, '-f', 'best[ext=mp4]', '-o', filepath, '--no-mtime'],
    {},
    controller.signal
  )

  async function cancel(id: any) {
    if (id && id === dbFile.id) {
      controller.abort('cancelled by user')
      dbFile.state = 'cancelled'
      await db
        .update(downloads)
        .set(omit(dbFile, 'id'))
        .where(eq(downloads.id, dbFile.id))
        .returning()
        .then(([newDbFile]) => {
          ytdlpEvents.emit('list', [newDbFile])
          ytdlpEvents.emit('status', {
            id: newDbFile.id,
            action: 'download',
            data: videoInfo,
            state: 'cancelled'
          })
          ytdlpEvents.emit('download', { id: newDbFile.id, percent: 100 })
        })

      ytdlpEvents.off('cancel', cancel)
    }
  }
  ytdlpEvents.on('cancel', cancel)
  ytdlpEvents.emit('list', [dbFile])
  ytdlpEvents.emit('download', { id: dbFile.id, percent: 0 })
  stream.on('progress', (ev) => {
    ytdlpEvents.emit('status', {
      id: dbFile.id,
      action: 'download',
      data: ev,
      state: 'progressing'
    })
    ytdlpEvents.emit('download', { ...ev, id: dbFile.id })
  })
  stream.once('progress', () => {
    ytdlpEvents.emit('list', [dbFile])
  })
  stream.once('close', () => {
    ytdlpEvents.emit('status', {
      id: dbFile.id,
      action: 'download',
      data: videoInfo,
      state: 'done'
    })
    ytdlpEvents.emit('download', { id: dbFile.id, percent: 100 })
  })
  stream.on('error', (error) => {
    ytdlpEvents.emit('status', { id: dbFile.id, action: 'download', error, state: 'error' })
    ytdlpEvents.emit('download', { id: dbFile.id, percent: 100, error })
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
  return await db
    .update(downloads)
    .set(omit(dbFile, 'id'))
    .where(eq(downloads.id, dbFile.id))
    .returning()
    .then(([newDbFile]) => newDbFile)
}
ytdlpEvents.on('add', handleYtdlMedia)
process.on('exit', () => {
  ytdlpEvents.off('add', handleYtdlMedia)
})
