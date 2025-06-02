import { and, desc, eq, inArray, InferInsertModel, InferSelectModel, isNotNull, like, not } from "drizzle-orm";
import { omit } from "lodash";
import { db } from "./app-database";
import { downloads } from "./app-database.schema";
export type SelectDownload = InferSelectModel<typeof downloads>;
export type InsertDownload = InferInsertModel<typeof downloads>;

function createDownload(item: InsertDownload) {
	return db.insert(downloads).values(item).returning();
}
function findDownloadByUrl(url: string, state?: string | string[]) {
	const itemState = [state].flat() as string[];
	return db
		.select()
		.from(downloads)
		.where(and(like(downloads.url, `${url}%`), itemState?.length ? inArray(downloads.state, itemState) : isNotNull(downloads.state)))
		.all();
}
function findDownloadByExactUrl(url: string, dbFileId?: SelectDownload["id"]) {
	return db
		.select()
		.from(downloads)
		.where(and(eq(downloads.url, url), isNotNull(downloads.meta), ...((dbFileId !== undefined && [not(eq(downloads.id, dbFileId))]) || [])))
		.orderBy(desc(downloads.created))
		.get();
}
function findDownloadById(dbFileId: SelectDownload["id"]) {
	return db
		.select()
		.from(downloads)
		.where(and(eq(downloads.id, dbFileId)))
		.orderBy(desc(downloads.created))
		.get();
}
function updateDownload(id: SelectDownload["id"], item: SelectDownload) {
	return db
		.update(downloads)
		.set(omit(item, "id"))
		.where(eq(downloads.id, id))
		.returning()
		.then(([s]) => s);
}
function deleteDownload(id: SelectDownload["id"]) {
	return db.delete(downloads).where(eq(downloads.id, id));
}

export const queries = {
	downloads: {
		createDownload,
		updateDownload,
		deleteDownload,
		findDownloadByUrl,
		findDownloadByExactUrl,
		findDownloadById,
	},
};
