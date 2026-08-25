import { existsSync, mkdirSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { mergeNetscapeCookieFiles, summarizeNetscapeCookies } from "@main/lib/cookies/netscape";
import { createEncryptedStore } from "@shared/electron/store/createYmlStore";
import { generateRandom } from "@shared/randomString";
import { app } from "electron";
import { z } from "zod";

export const cookieFileSchema = z.object({
	id: z.string(),
	name: z.string(),
	type: z.enum(["imported", "cookie"]),
	content: z.string(),
	createdAt: z.number(),
	updatedAt: z.number(),
});

export const cookiesStoreSchema = z
	.object({
		files: z.array(cookieFileSchema).fallback([]),
		activeFileIds: z.array(z.string()).fallback([]),
		isEnabled: z.boolean().fallback(true),
	})
	.fallback({ files: [], activeFileIds: [], isEnabled: true });

export type CookieFileRecord = z.infer<typeof cookieFileSchema>;
export type CookiesStoreData = z.infer<typeof cookiesStoreSchema>;

export type CookieFileMeta = Omit<CookieFileRecord, "content"> & {
	cookieCount: number;
	domainCount: number;
	domains: string[];
	isActive: boolean;
};

type CookiesStore = ReturnType<typeof createEncryptedStore<CookiesStoreData>>;

let storeInstance: CookiesStore | null = null;

/**
 * Created on first use instead of at import time, so requiring this module
 * during main process boot never touches disk or the encryption key.
 */
function store(): CookiesStore {
	if (!storeInstance) {
		storeInstance = createEncryptedStore<CookiesStoreData>("cookies", {
			defaults: cookiesStoreSchema.parse({ files: [], activeFileIds: [], isEnabled: true }),
		});
	}
	return storeInstance;
}

export function getYtdlpCookiesDir(): string {
	return join(app.getPath("userData"), "cookies");
}

export function getYtdlpCookiesPath(): string {
	return join(getYtdlpCookiesDir(), "ytdlp-cookies.txt");
}

export function getYtdlpCookiesFilePath(): string | null {
	if (!store().store.isEnabled || !store().store.activeFileIds.length) return null;
	const cookiesPath = getYtdlpCookiesPath();
	return existsSync(cookiesPath) ? cookiesPath : null;
}

export function toCookieFileMeta(file: CookieFileRecord, activeFileIds: string[]): CookieFileMeta {
	const summary = summarizeNetscapeCookies(file.content);
	return {
		id: file.id,
		name: file.name,
		type: file.type,
		createdAt: file.createdAt,
		updatedAt: file.updatedAt,
		cookieCount: summary.cookieCount,
		domainCount: summary.domainCount,
		domains: summary.domains.slice(0, 4),
		isActive: activeFileIds.includes(file.id),
	};
}

function uniqueFileName(name: string, files: CookieFileRecord[]): string {
	const existing = new Set(files.map((file) => file.name.toLowerCase()));
	if (!existing.has(name.toLowerCase())) return name;
	const extensionIndex = name.lastIndexOf(".");
	const base = extensionIndex > 0 ? name.slice(0, extensionIndex) : name;
	const extension = extensionIndex > 0 ? name.slice(extensionIndex) : "";
	let index = 2;
	let nextName = `${base} (${index})${extension}`;
	while (existing.has(nextName.toLowerCase())) {
		index += 1;
		nextName = `${base} (${index})${extension}`;
	}
	return nextName;
}

export function listCookieFiles(): CookieFileMeta[] {
	const { files, activeFileIds } = store().store;
	return files.map((file) => toCookieFileMeta(file, activeFileIds));
}

export function getCookieFile(id: string): CookieFileRecord | null {
	return store().store.files.find((file) => file.id === id) ?? null;
}

export function getActiveFileIds(): string[] {
	return store().store.activeFileIds;
}

export function getCookiesEnabled(): boolean {
	return store().store.isEnabled;
}

export function setCookiesEnabled(isEnabled: boolean): boolean {
	store().set("isEnabled", isEnabled);
	return isEnabled;
}

export function rewriteMergedCookiesFile(): string | null {
	const { files, activeFileIds } = store().store;
	const selected = activeFileIds
		.map((id) => files.find((file) => file.id === id))
		.filter((file): file is CookieFileRecord => Boolean(file));
	const cookiesPath = getYtdlpCookiesPath();
	if (!selected.length) {
		if (existsSync(cookiesPath)) unlinkSync(cookiesPath);
		return null;
	}
	const merged = mergeNetscapeCookieFiles(selected.map((file) => file.content));
	if (!summarizeNetscapeCookies(merged).cookieCount) {
		if (existsSync(cookiesPath)) unlinkSync(cookiesPath);
		return null;
	}
	mkdirSync(getYtdlpCookiesDir(), { recursive: true });
	writeFileSync(cookiesPath, merged, "utf8");
	return cookiesPath;
}

export function addCookieFiles(
	entries: { name: string; content: string; type?: CookieFileRecord["type"] }[],
): CookieFileMeta[] {
	const now = Date.now();
	const files = [...store().store.files];
	const created: CookieFileRecord[] = [];
	for (const entry of entries) {
		const record: CookieFileRecord = {
			id: generateRandom(16),
			name: uniqueFileName(entry.name, files),
			type: entry.type ?? "imported",
			content: entry.content,
			createdAt: now,
			updatedAt: now,
		};
		files.push(record);
		created.push(record);
	}
	store().set("files", files);
	return created.map((file) => toCookieFileMeta(file, store().store.activeFileIds));
}

export function updateCookieFile(id: string, patch: { content?: string; name?: string }): CookieFileRecord {
	const files = [...store().store.files];
	const index = files.findIndex((file) => file.id === id);
	if (index === -1) throw new Error("Cookie file not found");
	const current = files[index];
	const nextName =
		patch.name && patch.name !== current.name
			? uniqueFileName(
					patch.name,
					files.filter((file) => file.id !== id),
				)
			: current.name;
	const next: CookieFileRecord = {
		...current,
		name: nextName,
		content: patch.content ?? current.content,
		updatedAt: Date.now(),
	};
	files[index] = next;
	store().set("files", files);
	if (store().store.activeFileIds.includes(id)) rewriteMergedCookiesFile();
	return next;
}

export function deleteCookieFiles(ids: string[]): void {
	const idSet = new Set(ids);
	store().set(
		"files",
		store().store.files.filter((file) => !idSet.has(file.id)),
	);
	store().set(
		"activeFileIds",
		store().store.activeFileIds.filter((id) => !idSet.has(id)),
	);
	rewriteMergedCookiesFile();
}

export function activateCookieFiles(ids: string[]): string | null {
	const idSet = new Set(ids);
	const existingIds = store()
		.store.files.filter((file) => idSet.has(file.id))
		.map((file) => file.id);
	store().set("activeFileIds", existingIds);
	return rewriteMergedCookiesFile();
}
