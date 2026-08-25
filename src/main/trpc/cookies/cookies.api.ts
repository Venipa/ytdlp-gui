import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import { NETSCAPE_FILE_TEMPLATE } from "@main/lib/cookies/netscape";
import {
	activateCookieFiles,
	addCookieFiles,
	deleteCookieFiles,
	getActiveFileIds,
	getCookieFile,
	getCookiesEnabled,
	getYtdlpCookiesFilePath,
	listCookieFiles,
	setCookiesEnabled,
	toCookieFileMeta,
	updateCookieFile,
} from "@main/stores/cookies/cookies.store";
import { TRPCError } from "@trpc/server";
import { dialog } from "electron";
import { z } from "zod";
import { publicProcedure, router } from "../core/trpc";

export const cookiesRouter = router({
	list: publicProcedure.query(() => {
		return {
			files: listCookieFiles(),
			activeFileIds: getActiveFileIds(),
			isEnabled: getCookiesEnabled(),
			cookiesPath: getYtdlpCookiesFilePath(),
		};
	}),
	get: publicProcedure.input(z.object({ id: z.string() })).query(({ input: { id } }) => {
		const file = getCookieFile(id);
		if (!file) throw new TRPCError({ code: "NOT_FOUND", message: "Cookie file not found" });
		return {
			...toCookieFileMeta(file, getActiveFileIds()),
			content: file.content,
		};
	}),
	create: publicProcedure.mutation(() => {
		const [created] = addCookieFiles([
			{ name: "manual-cookies.txt", content: NETSCAPE_FILE_TEMPLATE, type: "cookie" },
		]);
		return created;
	}),
	importFiles: publicProcedure.mutation(async ({ ctx: { window } }) => {
		window.setAlwaysOnTop(true);
		const result = await dialog.showOpenDialog(window, {
			properties: ["openFile", "multiSelections"],
			title: "Import cookie files",
			filters: [
				{ name: "Cookie files", extensions: ["txt", "cookies"] },
				{ name: "All files", extensions: ["*"] },
			],
		});
		window.setAlwaysOnTop(false);
		if (result.canceled || !result.filePaths.length) {
			return { imported: [] as ReturnType<typeof listCookieFiles> };
		}
		const entries: { name: string; content: string }[] = [];
		for (const filePath of result.filePaths) {
			const content = await readFile(filePath, "utf8");
			entries.push({ name: basename(filePath), content });
		}
		return { imported: addCookieFiles(entries) };
	}),
	update: publicProcedure
		.input(
			z.object({
				id: z.string(),
				content: z.string().optional(),
				name: z.string().min(1).optional(),
			}),
		)
		.mutation(({ input: { id, content, name } }) => {
			try {
				const file = updateCookieFile(id, { content, name });
				return toCookieFileMeta(file, getActiveFileIds());
			} catch (error: unknown) {
				throw new TRPCError({
					code: "NOT_FOUND",
					message: error instanceof Error ? error.message : "Cookie file not found",
				});
			}
		}),
	delete: publicProcedure.input(z.object({ ids: z.array(z.string()).min(1) })).mutation(({ input: { ids } }) => {
		deleteCookieFiles(ids);
		return { ids };
	}),
	setEnabled: publicProcedure.input(z.boolean()).mutation(({ input: isEnabled }) => {
		return { isEnabled: setCookiesEnabled(isEnabled) };
	}),
	activate: publicProcedure.input(z.object({ ids: z.array(z.string()).min(1) })).mutation(({ input: { ids } }) => {
		const cookiesPath = activateCookieFiles(ids);
		return {
			ids,
			cookiesPath,
			files: listCookieFiles(),
		};
	}),
});
