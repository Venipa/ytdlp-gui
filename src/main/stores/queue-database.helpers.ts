import {
  and,
  eq,
  inArray,
  InferInsertModel,
  InferSelectModel,
  isNotNull,
  like
} from 'drizzle-orm'
import { omit } from 'lodash'
import { db } from './queue-database'
import { downloads } from './queue-database.schema'
type SelectDownload = InferSelectModel<typeof downloads>
type InsertDownload = InferInsertModel<typeof downloads>

function createDownload(item: InsertDownload) {
  return db.insert(downloads).values(item).returning()
}
function findDownloadByUrl(url: string, state?: string | string[]) {
  const itemState = [state].flat() as string[]
  return db
    .select()
    .from(downloads)
    .where(
      and(
        like(downloads.url, `${url}%`),
        itemState.length ? inArray(downloads.state, itemState) : isNotNull(downloads.state)
      )
    )
    .all()
}
function updateDownload(id: SelectDownload['id'], item: SelectDownload) {
  return db.update(downloads).set(omit(item, 'id')).where(eq(downloads.id, id)).returning()
}
function deleteDownload(id: SelectDownload['id']) {
  return db.delete(downloads).where(eq(downloads.id, id))
}

export const queries = {
  downloads: {
    createDownload,
    updateDownload,
    deleteDownload,
    findDownloadByUrl
  }
}
