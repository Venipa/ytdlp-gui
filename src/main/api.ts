import { router } from '@main/trpc/trpc';
import { inferRouterInputs, inferRouterOutputs } from '@trpc/server';
import { dialogRouter } from './trpc/dialog.api';
import { internalRouter } from './trpc/internal.api';
import { settingsRouter } from './trpc/settings.api';
import { windowRouter } from './trpc/window.api';
import { ytdlpRouter } from './trpc/ytdlp.api';


export const appRouter = router({
  window: windowRouter,
  dialog: dialogRouter,
  internals: internalRouter,
  ytdl: ytdlpRouter,
  settings: settingsRouter
});

export type AppRouter = typeof appRouter;

export type RouterInput = inferRouterInputs<AppRouter>;
export type RouterOutput = inferRouterOutputs<AppRouter>;
