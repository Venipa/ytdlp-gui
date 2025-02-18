import type { SoundStoreSource } from '@main/stores/sound.store'
import { trpc } from '@renderer/lib/trpc-link'
import { useSoundSet } from '@renderer/lib/useSoundSet'
import { Context, createContext, Provider, useContext } from 'react'
type ToneContext = {
  selected: SoundStoreSource | null | undefined
  setSound: (id: string) => Promise<SoundStoreSource | undefined>
  setVolume: (volume: number) => Promise<number>
  setEnabled: (enabled: boolean) => Promise<boolean>
  getSound: (id: string) => Promise<SoundStoreSource | undefined>
}
type ToneContextType = Context<ToneContext>
const toneContext: ToneContextType = createContext({
  selected: null
} as any)
const useTone = () => useContext(toneContext)

const ToneContextProvider: Provider<ToneContext> = (({ value, ...props }) => {
  const [data, _setSoundSet] = useSoundSet()
  const _setVolume = trpc.sounds.setVolume.useMutation().mutateAsync
  const _setEnabled = trpc.sounds.setEnabled.useMutation().mutateAsync
  const { sounds: utils } = trpc.useUtils()
  const getSound = (id: string) => {
    return utils.getSoundSet.fetch(id)
  }
  const setSound = (id: string) => {
    return _setSoundSet(id).then(() => {
      return utils.invalidate() as any;
    })
  }
  const setVolume = (volume: number) => {
    return _setVolume(volume).then(() => {
      return utils.invalidate() as any;
    })
  }
  const setEnabled = (enabled: boolean) => {
    return _setEnabled(enabled).then(() => {
      return utils.invalidate() as any;
    })
  }
  return (
    <toneContext.Provider
      value={{ selected: data, getSound, setSound, setVolume, setEnabled }}
      {...props}
    ></toneContext.Provider>
  )
}) as any
export { ToneContextProvider, useTone }
