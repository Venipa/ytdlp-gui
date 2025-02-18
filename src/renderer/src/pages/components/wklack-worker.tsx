import { logger } from '@shared/logger'
import { useEffect, useState } from 'react'
import { useTone } from './tone-context'
let soundPlayer: ReturnType<typeof window.api.createSoundSetPlayer>
export default function WKlack() {
  const { selected: soundSet } = useTone()
  const [player] = useState(() =>
    soundPlayer
      ? soundPlayer
      : (soundPlayer = window.api.createSoundSetPlayer({
          reuseAudioContext: window.api.platform.isMacOS
        }))
  )
  useEffect(() => {
    logger.debug('wKlack sound update', soundSet)
    if (!window.license?.activated) {
      if (player.isInitialized) player.destroy()
      return
    }
    if (soundSet?.enabled !== false) {
      if (!player.isInitialized) {
        player.initialize()
      }
      player.loadSoundSet(soundSet as any)
      if (soundSet?.volume !== undefined) {
        player.updateVolume(soundSet.volume)
      }
      if (soundSet?.echo !== undefined) {
        player.setEcho(soundSet?.echo !== false)
      }
    } else if (soundSet?.enabled === false) {
      if (player.isInitialized) player.destroy()
    }
    window.addEventListener('beforeunload', () => {
      player?.destroy()
    })
  }, [soundSet])
  return null
}
