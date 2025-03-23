import { db } from '@main/stores/app-database'
import { queries, SelectDownload } from '@main/stores/app-database.helpers'
import { downloads } from '@main/stores/app-database.schema'
import { appStore } from '@main/stores/app.store'
import { logger } from '@shared/logger'
import { queuePromiseStack } from '@shared/promises/helper'
import { TRPCError } from '@trpc/server'
import { observable } from '@trpc/server/observable'
import { desc } from 'drizzle-orm'
import { statSync } from 'fs'
import { omit, uniq } from 'lodash'
import path from 'path'
import { VideoInfo } from 'yt-dlp-wrap/types'
import { YTDLDownloadStatus, YTDLItem, YTDLStatus } from 'ytdlp-gui/types'
import { z } from 'zod'
import { pushLogToClient } from './events.ee'
import { publicProcedure, router } from './trpc'
import {
  MAX_PARALLEL_DOWNLOADS,
  MAX_STREAM_CONCURRENT_FRAGMENTS,
  ytdl,
  YTDLP_CACHE_PATH
} from './ytdlp.core'
import { ytdlpDownloadQueue, ytdlpEvents } from './ytdlp.ee'
const log = logger.child('ytdlp.api')
export const ytdlpRouter = router({
  state: publicProcedure.query(() => ytdl.state.toString()),
  checkUpdates: publicProcedure.mutation(() => ytdl.checkUpdates()),
  downloadMedia: publicProcedure
    .input(
      z.object({
        url: z.string().url().array(),
        type: z.enum(['video', 'audio', 'auto']).default('auto')
      })
    )
    .mutation(async ({ input: { url: urls }, ctx }) => {
      const dbEntries = await addToDatabase(urls)
      const files = await queuePromiseStack(
        dbEntries.map((u) => () => queueYtdlMetaCheck(u).catch(() => null)),
        MAX_PARALLEL_DOWNLOADS
      ).then((files) => files.filter((s) => !!s))
      const asyncResult = ytdlpDownloadQueue.addAll(
        files.map(
          (f) => () =>
            queueYtdlDownload(f.dbFile, f.videoInfo).catch((err) => {
              log.error('failed to download media', err)
              return Promise.reject(err)
            })
        )
      )
      if (ytdlpDownloadQueue.isPaused) ytdlpDownloadQueue.start()
      return await asyncResult
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
  delete: publicProcedure.input(z.number()).mutation(async ({ input: id }) => {
    const result = await queries.downloads.deleteDownload(id)
    if (!result.rowsAffected)
      throw new TRPCError({ code: 'NOT_FOUND', message: 'id not found in database' })
    ytdlpEvents.emit('list', [
      {
        id: id,
        state: 'deleted'
      }
    ])
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
  onAutoAdd: publicProcedure.subscription(() => {
    return observable<string>((emit) => {
      async function onAutoAddHandle(url: string) {
        if (
          !url ||
          (
            await queries.downloads.findDownloadByUrl(url, [
              'completed',
              'fetching_meta',
              'downloading'
            ])
          ).length
        ) {
          ytdlpEvents.emit('toast', { message: 'File already downloaded.', type: 'error' })
          return
        }
        ytdlpEvents.emit('autoAddCapture', url)
        const { value: videoInfo, error: videoInfoError } = await ytdl.getVideoInfo(url)
        if (videoInfoError || !videoInfo?.url) return
        emit.next(url)
      }

      ytdlpEvents.on('autoAdd', onAutoAddHandle)

      return () => {
        ytdlpEvents.off('autoAdd', onAutoAddHandle)
      }
    })
  }),
  onAutoAddCapture: publicProcedure.subscription(() => {
    return observable<{ url: string }>((emit) => {
      async function onAutoAddCaptureHandle(url: string) {
        emit.next({ url: new URL(url).toString() })
      }

      ytdlpEvents.on('autoAddCapture', onAutoAddCaptureHandle)

      return () => {
        ytdlpEvents.off('autoAddCapture', onAutoAddCaptureHandle)
      }
    })
  }),
  onToast: publicProcedure.subscription(() => {
    return observable<string[]>((emit) => {
      async function onToastRelay(
        data: string | { message: string; type?: string; description?: string }
      ) {
        emit.next(
          typeof data === 'string'
            ? [data]
            : ([data.message, data.description, data.type] as string[])
        )
      }

      ytdlpEvents.on('toast', onToastRelay)

      return () => {
        ytdlpEvents.off('toast', onToastRelay)
      }
    })
  }),
  stats: publicProcedure.query(async () => {
    const items = await db.select().from(downloads).all()
    return items.reduce(
      (acc, r) => {
        if (r.state === 'completed') acc.overallCount++
        const type = r.type?.toLowerCase()
        if (type && r.state === 'completed') acc.count[type]++
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

const deleteDownloadItem = (dbFile: SelectDownload) =>
  queries.downloads.deleteDownload(dbFile.id).then((s) => {
    if (s.rowsAffected === 1) {
      dbFile.state = 'deleted'
      ytdlpEvents.emit('list', [dbFile])

      ytdlpEvents.emit('status', {
        id: dbFile.id,
        action: 'download',
        data: null,
        state: 'deleted'
      })
    }
  })
const addToDatabase = async (urls: string[]) => {
  const [items] = await db.batch(
    urls
      .map((url) => {
        if (typeof url !== 'string' || !/^https/gi.test(url)) return null
        return queries.downloads.createDownload({
          filepath: '',
          filesize: 0,
          meta: {} as any,
          metaId: '',
          source: new URL(url).hostname,
          title: url,
          url,
          state: 'queued',
          type: null,
          error: null,
          retryCount: 0
        })
      })
      .filter(Boolean) as any
  )
  ytdlpEvents.emit('list', items)
  log.debug('addToDatabase', { items })
  return items as SelectDownload[]
}
const queueYtdlMetaCheck = async (
  createdDbFile: SelectDownload
): Promise<{ dbFile: SelectDownload; videoInfo: VideoInfo }> => {
  const { url } = createdDbFile
  log.debug('meta', `added url`, url, createdDbFile)
  if (!createdDbFile.url) throw new Error('Invalid url format')
  ytdlpEvents.emit('status', { action: 'getVideoInfo', state: 'progressing' })
  let dbFile = await queries.downloads.findDownloadById(createdDbFile.id)
  if (!dbFile) throw new Error('Entry has been not found or has been removed')
  const existingDbFile = await queries.downloads.findDownloadByExactUrl(url, dbFile.id)
  Object.assign(dbFile, {
    metaId: existingDbFile?.metaId ?? '',
    meta: existingDbFile?.meta ?? ({} as any),
    filepath: existingDbFile?.filepath ?? '',
    filesize: existingDbFile?.filesize ?? 0,
    source: new URL(url).hostname,
    state: 'fetching_meta',
    title: existingDbFile?.title ?? url,
    type: null,
    error: null,
    retryCount: 0
  })
  await queries.downloads.updateDownload(dbFile.id, dbFile)
  ytdlpEvents.emit('list', [dbFile])
  if (!dbFile.meta?.filename) {
    const { value: videoInfo, error: videoInfoError } = await ytdl.getVideoInfo(url)
    log.debug('metadata', omit(videoInfo, 'thumbnails', 'formats'))
    if (videoInfoError || !videoInfo) {
      if (videoInfoError) log.error('getVideoInfo', videoInfoError)
      await deleteDownloadItem(dbFile)
      throw new TRPCError({
        code: 'NOT_FOUND',
        message: 'URL not supported or video not found'
      })
    }
    dbFile.meta = omit(videoInfo, 'formats', 'thumbnails') as VideoInfo
    dbFile.metaId = videoInfo.id
    pushLogToClient(`[${dbFile.id}=${dbFile.metaId}] added new download: ${dbFile.title}`, 'info')
    dbFile = await queries.downloads.updateDownload(dbFile.id, dbFile)
    return { dbFile, videoInfo }
  }
  return { dbFile, videoInfo: dbFile.meta }
}
const queueYtdlDownload = async (dbFile: SelectDownload, videoInfo: VideoInfo) => {
  if (!ytdl.currentDownloadPath)
    throw new TRPCError({
      code: 'NOT_FOUND',
      message: 'YTDLP has not been found, make sure the app is running with sufficient permission.'
    })
  const controller = new AbortController()
  const deleteEntry = () => deleteDownloadItem(dbFile)
  const updateEntry = () =>
    queries.downloads.updateDownload(dbFile.id, dbFile).then((s) => {
      ytdlpEvents.emit('list', [s])
      dbFile = s
      return s
    })
  if (controller.signal.aborted) {
    await deleteEntry()
    throw new TRPCError({ code: 'CLIENT_CLOSED_REQUEST', message: 'Video fetch aborted' })
  }
  const settings = appStore.store.ytdlp
  ytdlpEvents.emit('status', { action: 'getVideoInfo', state: 'done' })
  const filepath = path.join(ytdl.currentDownloadPath, videoInfo.filename)
  dbFile.meta = omit(videoInfo, ['formats']) as any
  dbFile.metaId = videoInfo.id
  dbFile.filepath = filepath
  dbFile.title = videoInfo.title
  dbFile.state = 'downloading'
  dbFile.filesize = videoInfo.filesize_approx ?? videoInfo.filesize ?? 0
  dbFile.type = videoInfo._type?.toLowerCase() ?? 'video'
  await updateEntry()
  const execArgs = [
    dbFile.url,
    '-f',
    'best',
    '-o',
    filepath,
    '--concurrent-fragments',
    String(MAX_STREAM_CONCURRENT_FRAGMENTS),
    '--cache-dir',
    YTDLP_CACHE_PATH
  ]
  if (settings.flags?.nomtime) execArgs.push('--no-mtime')
  if (settings.flags?.custom) execArgs.push(...settings.flags.custom.split(' '))
  const stream = ytdl.ytdlp.exec(uniq(execArgs), {}, controller.signal)

  async function cancel(id: any) {
    if (id && id === dbFile.id) {
      controller.abort('cancelled by user')
      dbFile.state = 'cancelled'
      await updateEntry()
      ytdlpEvents.emit('list', [dbFile])
      ytdlpEvents.emit('status', {
        id: dbFile.id,
        action: 'download',
        data: videoInfo,
        state: 'cancelled'
      })
      ytdlpEvents.emit('download', { id: dbFile.id, percent: 100 })

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
  pushLogToClient(`[${dbFile.id}=${dbFile.metaId}] finished download: ${dbFile.title}`, 'success')
  return await updateEntry()
}
async function handleYtAddEvent(url: string) {
  const [newDbFile] = await addToDatabase([url])
  await queueYtdlMetaCheck(newDbFile).then(({ dbFile, videoInfo }) => {
    const asyncResult = ytdlpDownloadQueue.add(() => queueYtdlDownload(dbFile, videoInfo))
    if (ytdlpDownloadQueue.isPaused) ytdlpDownloadQueue.start()
    return asyncResult
  })
}
ytdlpEvents.on('add', handleYtAddEvent)
process.on('exit', () => {
  ytdlpEvents.off('add', handleYtAddEvent)
})
