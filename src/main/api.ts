import { publicProcedure, router } from '@main/trpc/trpc';
import { inferRouterInputs, inferRouterOutputs } from '@trpc/server';
import { observable } from '@trpc/server/observable';
import { EventEmitter } from 'events';
import z from 'zod';
import { dialogRouter } from './trpc/dialog.api';
import { internalRouter } from './trpc/internal.api';
import { soundRouter } from './trpc/sound.api';
import { windowRouter } from './trpc/window.api';

const ee = new EventEmitter();

export const appRouter = router({
  window: windowRouter,
  dialog: dialogRouter,
  internals: internalRouter,
  sounds: soundRouter,
  greeting: publicProcedure.input(z.object({ name: z.string() })).query((req) => {
    const { input } = req;

    ee.emit('greeting', `Greeted ${input.name}`);
    return {
      text: `Hello ${input.name}` as const,
    };
  }),
  subscription: publicProcedure.subscription(() => {
    return observable((emit) => {
      function onGreet(text: string) {
        emit.next({ text });
      }

      ee.on('greeting', onGreet);

      return () => {
        ee.off('greeting', onGreet);
      };
    });
  }),
});

export type AppRouter = typeof appRouter;

export type RouterInput = inferRouterInputs<AppRouter>;
export type RouterOutput = inferRouterOutputs<AppRouter>;
