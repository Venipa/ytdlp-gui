// import { appStore } from '@main/stores/app.store'
// import { observable } from '@trpc/server/observable'
// import EventEmitter from 'events'
// import { z } from 'zod'
// import { publicProcedure, router } from './trpc'
// const settingsChangeEmitter = new EventEmitter()
// const handleKey = `settings_change`
// export const settingsRouter = router({
//   index: publicProcedure.query(async () => {
//     return appStore.store
//   }),
//   key: publicProcedure.input(z.string()).query(async ({ input: key }) => {
//     return appStore.get(key) as any
//   }),
//   onChange: publicProcedure.subscription(({ ctx }) => {
//     let handle: any
//     let unsubscribeStore = appStore.onDidAnyChange((c) =>
//       settingsChangeEmitter.emit(handleKey, { key: c })
//     )
//     return observable<Settings>((emit) => {
//       settingsChangeEmitter.on(
//         handleKey,
//         (handle = (value) => {
//           ctx.log.debug('change', value)
//           emit.next(value)
//         })
//       )
//       return () => {
//         if (handle) settingsChangeEmitter.off(handleKey, handle)
//         unsubscribeStore()
//       }
//     })
//   }),
//   update: publicProcedure
//     .input(
//       z.object({
//         key: z.string(),
//         value: z.union([z.any(), z.null()])
//       })
//     )
//     .mutation(async ({ ctx, input: { key, value } }) => {
//       ctx.log.debug({ key, value })
//       appStore.set(key, value);
//       return true;
//     })
// })
