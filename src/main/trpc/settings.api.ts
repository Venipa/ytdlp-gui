import EventEmitter from "events";
import { AppStore } from "@main/stores/AppStore";
import { appStore } from "@main/stores/app.store";
import config from "@shared/config";
import { TRPCError } from "@trpc/server";
import { observable } from "@trpc/server/observable";
import { app, dialog } from "electron";
import { uniqBy } from "lodash-es";
import { z } from "zod";
import { publicProcedure, router } from "./trpc";
const settingsChangeEmitter = new EventEmitter();
const handleKey = `settings_change`;
export const settingsRouter = router({
	index: publicProcedure.query(async () => {
		return appStore.store;
	}),
	key: publicProcedure.input(z.string()).query(async ({ input: key }) => {
		return appStore.get(key) as any;
	}),
	onChange: publicProcedure.subscription(({ ctx }) => {
		let handle: any;
		let unsubscribeStore = appStore.onDidAnyChange((c) => {
			settingsChangeEmitter.emit(handleKey, { key: c });
		});
		return observable<AppStore>((emit) => {
			settingsChangeEmitter.on(
				handleKey,
				(handle = (value) => {
					ctx.log.debug("change", value);
					if (
						appStore.store.download.paths?.length &&
						(!appStore.store.download.selected || !appStore.store.download.paths.find((d) => d === appStore.store.download.selected))
					) {
						appStore.store.download.selected = appStore.store.download.paths[0];
					}
					emit.next(appStore.store);
				}),
			);
			return () => {
				if (handle) settingsChangeEmitter.off(handleKey, handle);
				unsubscribeStore();
			};
		});
	}),
	update: publicProcedure
		.input(
			z.object({
				key: z.string(),
				value: z.union([z.any(), z.null()]),
			}),
		)
		.mutation(async ({ ctx, input: { key, value } }) => {
			ctx.log.debug({ key, value });
			appStore.set(key, value);
			settingsChangeEmitter.emit(handleKey, { key });
			return { key, value };
		}),
	updatePartial: publicProcedure
		.input(
			z
				.object({
					key: z.string(),
					value: z.any(),
				})
				.array(),
		)
		.mutation(async ({ ctx, input }) => {
			await Promise.allSettled(input.map(({ key, value }) => appStore.set(key, value)));
			uniqBy(input, (s) => s.key.slice(0, s.key.indexOf("."))).forEach(({ key }) => settingsChangeEmitter.emit(handleKey, { key }));
			return { success: true };
		}),
	addDownloadPath: publicProcedure.mutation(async ({ ctx: { window } }) => {
		window.setAlwaysOnTop(true);
		const folder = await dialog.showOpenDialog(window, {
			properties: ["openDirectory"],
			title: "Select new download path to " + config.title,
			defaultPath: appStore.store.download.selected || app.getPath("downloads"),
		});
		if (!folder || folder.canceled || !folder.filePaths.length) {
			window.setAlwaysOnTop(false);
			throw new TRPCError({ code: "BAD_REQUEST", message: "Missing or Invalid Folder Path" });
		}
		const folderPath = folder.filePaths[0];
		if (appStore.store.download.paths.find((d) => d.toLowerCase() === folderPath.toLowerCase())) {
			window.setAlwaysOnTop(false);
			throw new TRPCError({
				code: "CONFLICT",
				message: "Folder Path already exists in " + config.title,
			});
		}
		appStore.set("download", {
			paths: [...appStore.store.download.paths, folderPath],
			selected: folderPath,
		});
		settingsChangeEmitter.emit(handleKey, { key: "download" });
		window.setAlwaysOnTop(false);
	}),
	deleteDownloadPath: publicProcedure
		.input(z.string().refine((d) => appStore.store.download.paths.find((p) => p.toLowerCase() === d.toLowerCase()), "Path does not exist in database."))
		.mutation(async ({ ctx: { window }, input: downloadPath }) => {
			appStore.set(
				"download.paths",
				appStore.store.download.paths.filter((p) => p.toLowerCase() !== downloadPath.toLowerCase()),
			);
			settingsChangeEmitter.emit(handleKey, { key: "download.paths" });
		}),
});
