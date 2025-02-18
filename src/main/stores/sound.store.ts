import soundSetPaths from '@main/lib/audioModules'
import { SoundSource } from '@main/lib/soundTypes'
import { createEncryptedStore } from '@shared/electron/store/createYmlStore'
const soundSets = soundSetPaths

interface Sound {
  file: string
  echo?: boolean
}
export interface SoundSourceConfig {
  echo?: boolean
  volume?: number
  enabled?: boolean
  distinct?: boolean
}
export interface SoundStoreSource extends SoundSource, SoundSourceConfig {}
export interface SoundStore {
  active: SoundStoreSource | null | undefined
  configs: Record<string, SoundSourceConfig>
}
const store = createEncryptedStore<SoundStore>('app-sounds', {
  defaults: {
    active: {
      ...soundSets!.find(d => d.key === "alpaca"),
      enabled: true,
      volume: 10,
      distinct: true,
      echo: false
    },
    configs: {}
  }
})

export { soundSets, store as soundStore }
