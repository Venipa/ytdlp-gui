import { db } from '@main/stores/queue-database'
import { downloads } from '@main/stores/queue-database.schema'
import { logger } from '@shared/logger'
import { inArray } from 'drizzle-orm'
import { app } from 'electron'
import { clamp } from 'lodash'
import { availableParallelism } from 'os'
import path from 'path'
import { YTDLP } from './ytdlp.utils'

export const ytdl = new YTDLP()

export const checkBrokenLinks = async () => {
  const itemCount = await db
    .update(downloads)
    .set({ state: 'cancelled' })
    .where(inArray(downloads.state, ['downloading', 'fetching_meta']))
  logger.info(`Updated state of ${itemCount.rowsAffected} to cancelled`)
}

export const MAX_PARALLEL_TASKS = availableParallelism()
export const MAX_STREAM_CONCURRENT_FRAGMENTS = clamp(MAX_PARALLEL_TASKS, 1, 6)
export const YTDLP_CACHE_PATH = path.join(app.getPath('userData'), 'ytdlp_cache')
