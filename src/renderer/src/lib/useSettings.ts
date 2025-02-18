// import { RouterOutput } from '@main/api'
// import { UseTRPCQueryResult } from '@trpc/react-query/dist/shared'
// import { startTransition, useCallback } from 'react'
// import { trpc } from './trpc-link'

// export function useSettings() {
//   const data = trpc.settings.index.useQuery(undefined, {
//     refetchOnWindowFocus: false
//   })
//   return data
// }

// export function useSetting<
//   T = any,
//   R = [T, (value: T) => Promise<void>, UseTRPCQueryResult<RouterOutput['settings']['key'], any>]
// >(key: string, defaultValue?: T): R {
//   const [data] = trpc.settings.key.useSuspenseQuery(key, {
//     queryKey: ["settings.key", key],
//     refetchOnWindowFocus: false
//   })
//   const utils = trpc.useUtils().settings
//   trpc.settings.onChange.useSubscription(undefined, {
//     onData(newData) {
//       if (newData) utils.key.setData(newData.key, newData)
//     }
//   })
//   const updater = useUpdateSetting(key)
//   const mutateData = useCallback((value: T) => updater(value), [updater, key])
//   return [data.data?.value === undefined ? defaultValue : data.data.value, mutateData, data] as any
// }

// export function useUpdateSetting<T, R = T>(key: string): (value: T) => Promise<R> {
//   const { mutateAsync } = trpc.settings.update.useMutation()
//   const { settings } = trpc.useUtils()
//   return (value: T) =>
//     new Promise<R>((resolve, reject) => {
//       startTransition(() => {
//         mutateAsync({ key, value })
//           .then((newData) => {
//             settings.key.setData(key, newData)
//             settings.index.setData(undefined, (s) => {
//               if (!s) return s
//               const idx = s.findIndex((d) => d.key === newData.key)
//               if (idx !== -1) s.splice(idx, 1, newData)
//               return s
//             })
//             return newData as R
//           })
//           .then(resolve)
//           .catch(reject) as Promise<R>
//       })
//     })
// }

// export function useSettingsDialog() {
//   const { mutateAsync } = trpc.dialog.settings.useMutation()
//   return { openDialog: () => mutateAsync() }
// }
