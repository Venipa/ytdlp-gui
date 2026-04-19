import { router } from "@main/trpc/core/trpc";
import dependenciesRouter from "@main/trpc/dependencies";
import { inferRouterInputs, inferRouterOutputs } from "@trpc/server";
import { dialogRouter } from "./trpc/dialog/dialog.api";
import { eventsRouter } from "./trpc/events/events.api";
import { internalRouter } from "./trpc/internal/internal.api";
import { settingsRouter } from "./trpc/settings/settings.api";
import { windowRouter } from "./trpc/window/window.api";
import { ytdlpRouter } from "./trpc/ytdlp/ytdlp.api";

export const appRouter = router({
	dependencies: dependenciesRouter,
	window: windowRouter,
	dialog: dialogRouter,
	internals: internalRouter,
	ytdl: ytdlpRouter,
	settings: settingsRouter,
	events: eventsRouter,
});

export type AppRouter = typeof appRouter;

export type RouterInput = inferRouterInputs<AppRouter>;
export type RouterOutput = inferRouterOutputs<AppRouter>;
