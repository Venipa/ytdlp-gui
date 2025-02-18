import secureStore from '@main/secureStore'
import { shell } from 'electron'
import { z } from 'zod'
import { mainProcedure, publicProcedure, router } from './trpc'
export const internalRouter = router({
  getAll: mainProcedure.query(() => secureStore.getAll()),
  set: mainProcedure
    .input(
      z.object({
        key: z.string(),
        value: z.string()
      })
    )
    .mutation(({ input: { key, value } }) => {
      return secureStore.set(key, value)
    }),
  get: mainProcedure.input(z.string()).query(({ input: key }) => {
    return secureStore.get(key)
  }),
  delete: mainProcedure.input(z.string()).mutation(({ input: key }) => {
    return secureStore.delete(key)
  }),
  setJson: mainProcedure
    .input(
      z.object({
        key: z.string(),
        value: z.union([z.object({}), z.boolean(), z.number(), z.null()])
      })
    )
    .mutation(({ input: { key, value } }) => {
      return secureStore.set(key, value)
    }),
  getJson: mainProcedure.input(z.string()).query(({ input: key }) => {
    return secureStore.get(key)
  }),
  openPath: publicProcedure.input(z.string()).mutation(async ({ input: filePath }) => {
    await shell.openPath(filePath)
  })
} as const)
