import { sql } from 'drizzle-orm'
import { blob, int, sqliteTable, text } from 'drizzle-orm/sqlite-core'
import { VideoInfo } from 'yt-dlp-wrap/types'

export const downloads = sqliteTable('downloads', {
  id: int({ mode: 'number' }).primaryKey({ autoIncrement: true }),
  title: text().notNull(),
  state: text(),
  filepath: text().notNull(),
  filesize: int({ mode: 'number' }),
  type: text(),
  url: text().notNull(),
  source: text().notNull(),
  retryCount: int({ mode: 'number' }),
  error: blob({ mode: 'json' }).default(null),
  meta: blob({ mode: 'json' }).$type<VideoInfo | null>().default(null),
  metaId: text().notNull(),
  created: text()
    .notNull()
    .default(sql`(current_timestamp)`)

  // id: string
  // state: string;
  // title: string
  // filesize: number
  // type: string
  // source: string
  // url: string
  // filepath: string
  // retryCount: number
  // error?: any
})
