import { dirname } from "node:path";
import secureStore from "@main/secureStore";
import { appStore } from "@main/stores/app.store";
import { checkForUpdates, checkForUpdatesAndNotify, setUpdateHandledByFrontend } from "@main/updater";
import { TRPCError } from "@trpc/server";
import { shell } from "electron";
import { autoUpdater } from "electron-updater";
import { z } from "zod";
import { mainProcedure, publicProcedure, router } from "./trpc";
import { ytdl } from "./ytdlp.core";
import { YTDLP_STATE } from "./ytdlp.utils";
let appInitialized = false;
export const internalRouter = router({
	getAll: mainProcedure.query(() => secureStore.getAll()),
	set: mainProcedure
		.input(
			z.object({
				key: z.string(),
				value: z.string(),
			}),
		)
		.mutation(({ input: { key, value } }) => {
			return secureStore.set(key, value);
		}),
	get: mainProcedure.input(z.string()).query(({ input: key }) => {
		return secureStore.get(key);
	}),
	delete: mainProcedure.input(z.string()).mutation(({ input: key }) => {
		return secureStore.delete(key);
	}),
	setJson: mainProcedure
		.input(
			z.object({
				key: z.string(),
				value: z.union([z.object({}), z.boolean(), z.number(), z.null()]),
			}),
		)
		.mutation(({ input: { key, value } }) => {
			return secureStore.set(key, value);
		}),
	getJson: mainProcedure.input(z.string()).query(({ input: key }) => {
		return secureStore.get(key);
	}),
	openPath: publicProcedure
		.input(
			z.object({
				path: z.string(),
				openParent: z.boolean().default(false),
			}),
		)
		.mutation(async ({ input: { path: filePath, openParent } }) => {
			if (openParent) shell.openPath(dirname(filePath));
			else shell.showItemInFolder(filePath);
		}),
	openFile: publicProcedure
		.input(
			z.object({
				path: z.string(),
			}),
		)
		.mutation(async ({ input: { path: filePath } }) => {
			await shell.openPath(filePath);
		}),
	checkUpdate: publicProcedure.mutation(async () => {
		return await checkForUpdates();
	}),
	downloadUpdate: publicProcedure.mutation(() => {
		try {
			setUpdateHandledByFrontend(true);
			return autoUpdater.downloadUpdate().catch((err) => {
				setUpdateHandledByFrontend(false);
				throw err;
			});
		} catch (ex: any) {
			throw new TRPCError({ message: ex.message, code: "INTERNAL_SERVER_ERROR" });
		}
	}),
	quitAndInstallUpdate: publicProcedure.mutation(() => {
		try {
			return autoUpdater.quitAndInstall(false, true);
		} catch (ex: any) {
			throw new TRPCError({ message: ex.message, code: "INTERNAL_SERVER_ERROR" });
		}
	}),
	initializeApp: publicProcedure.mutation(async () => {
		if (appInitialized) throw new TRPCError({ message: "App already initialized", code: "INTERNAL_SERVER_ERROR" });
		await ytdl.initialize();
		await ytdl.checkUpdates(ytdl.state === YTDLP_STATE.MISSING_BINARY);
		await checkForUpdatesAndNotify();
		appInitialized = true;
		return appStore.store.ytdlp.version ? appStore.store.ytdlp.version : await ytdl.ytdlp.getVersion();
	}),
} as const);
