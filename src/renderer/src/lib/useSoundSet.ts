import { RouterOutput } from '@main/api'
import { UseTRPCQueryResult } from '@trpc/react-query/dist/shared'
import { trpc } from './trpc-link'

export function useSoundSet<
  T = RouterOutput['sounds']['getActiveSoundSet'],
  R = [
    T,
    (value: string) => Promise<void>,
    UseTRPCQueryResult<RouterOutput['sounds']['getActiveSoundSet'], any>
  ]
>(): R {
  const [data] = trpc.sounds.getActiveSoundSet.useSuspenseQuery(undefined, {})
  const utils = trpc.useUtils().sounds
  const { mutateAsync } = trpc.sounds.setSoundSet.useMutation()
  trpc.sounds.onSoundSetChange.useSubscription(undefined, {
    onData(newData) {
      if (newData) {
        utils.getActiveSoundSet.setData(undefined, newData)
      }
      console.log('changed sound set', newData)
    }
  })
  return [data, mutateAsync] as any
}
