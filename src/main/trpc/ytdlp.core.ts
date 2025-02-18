import { db } from '@main/stores/queue-database'
import { downloads } from '@main/stores/queue-database.schema'
import { logger } from '@shared/logger'
import { eq } from 'drizzle-orm'
import { YTDLP } from './ytdlp.utils'

export const ytdl = new YTDLP()

export const checkBrokenLinks = async () => {
  const itemCount = await db
    .update(downloads)
    .set({ state: 'cancelled' })
    .where(eq(downloads.state, 'downloading'))
  logger.info(`Updated state of ${itemCount.rowsAffected} to cancelled`)
}
