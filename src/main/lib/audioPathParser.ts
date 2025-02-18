import { logger } from '@shared/logger'
import { fileURLToPath } from 'url'
export function parseAudioRoot(name: string) {
  const basePath = fileURLToPath(new URL('../../resources/audio/' + name, import.meta.url))
  logger.debug('audioModules', { basePath })
  return basePath
}
