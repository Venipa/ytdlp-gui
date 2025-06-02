import { initTRPC, TRPCError } from "@trpc/server";
import { BrowserWindow, IpcMainInvokeEvent } from "electron";
import { Logger } from "~/src/shared/logger";
import { pushLogToClient } from "./events.ee";
const t = initTRPC.context<{ window: BrowserWindow; event: IpcMainInvokeEvent; log: Logger; path: string }>().create({ isServer: true });
export const router = t.router;
/**
 * procedures that are allowed to be used everywhere
 */
export const publicProcedure = t.procedure.use(async ({ ctx, path, type, next }) => {
	const log = new Logger(path).child(type);
	ctx.log = log;
	ctx.path = path;
	return next({ ctx }).catch((err) => {
		if (err instanceof TRPCError) pushLogToClient(err.message, "error");
		return Promise.reject(err);
	});
});

/**
 * procedure that only runs on main process
 */
export const mainProcedure = t.procedure.use(async ({ ctx, next }) => {
	if (ctx.window) throw new Error("Invalid Context, caller must be main process");
	return next({ ctx });
});
