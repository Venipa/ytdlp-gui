import secureStore from '@main/secureStore'
import { TRPCError } from '@trpc/server'
import { shell } from 'electron'
import { autoUpdater } from 'electron-updater'
import { dirname } from 'node:path'
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
  openPath: publicProcedure
    .input(
      z.object({
        path: z.string(),
        openParent: z.boolean().default(false)
      })
    )
    .mutation(async ({ input: { path: filePath, openParent } }) => {
      if (openParent) shell.openPath(dirname(filePath))
      else shell.showItemInFolder(filePath)
    }),
  openFile: publicProcedure
    .input(
      z.object({
        path: z.string()
      })
    )
    .mutation(async ({ input: { path: filePath } }) => {
      await shell.openPath(filePath)
    }),
  checkUpdate: publicProcedure
    .input(z.boolean().default(false))
    .mutation(async ({ input: notifyIfUpdateAvailable }) => {
      if (notifyIfUpdateAvailable) return await autoUpdater.checkForUpdatesAndNotify()
      return await autoUpdater.checkForUpdates()
    }),
  downloadUpdate: publicProcedure.mutation(() => {
    try {
      return autoUpdater.downloadUpdate()
    } catch (ex: any) {
      throw new TRPCError({ message: ex.message, code: 'INTERNAL_SERVER_ERROR' })
    }
  }),
  quitAndInstallUpdate: publicProcedure.mutation(() => {
    try {
      return autoUpdater.quitAndInstall()
    } catch (ex: any) {
      throw new TRPCError({ message: ex.message, code: 'INTERNAL_SERVER_ERROR' })
    }
  })
} as const)
