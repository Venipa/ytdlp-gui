import { SoundSource } from '../soundTypes'

const soundSetPaths = Object.entries(
  import.meta.glob<{ default: SoundSource }>('/src/main/lib/audioModules/*.ts', { eager: true })
).map(([modulePath, mod]) => {
  return { ...mod.default, __modulePath: modulePath } as SoundSource
})
export default soundSetPaths
