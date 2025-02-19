import { db } from '@main/stores/queue-database'
import { downloads } from '@main/stores/queue-database.schema'
import { logger } from '@shared/logger'
import { inArray } from 'drizzle-orm'
import { YTDLP } from './ytdlp.utils'

export const ytdl = new YTDLP()

export const checkBrokenLinks = async () => {
  const itemCount = await db
    .update(downloads)
    .set({ state: 'cancelled' })
    .where(inArray(downloads.state, ['downloading', 'fetching_meta']))
  logger.info(`Updated state of ${itemCount.rowsAffected} to cancelled`)
}
