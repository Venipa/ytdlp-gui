import { ElectronAPI } from '@electron-toolkit/preload'
import type { platform } from '@electron-toolkit/utils'
import type { ProgressInfo, UpdateDownloadedEvent, UpdateInfo } from 'electron-updater'
import type Unlock from './license'
import type { SoundSetOptions, SoundSetPlayer } from './player'
declare global {
  interface Window {
    electron: ElectronAPI
    api: {
      createSoundSetPlayer(options?: SoundSetOptions): SoundSetPlayer
      version: string
      platform: typeof platform
      on(eventName: string, handle: (ev: IpcRendererEvent, ...args: any[]) => void): void
      invoke<T = any>(actionName: string, ...args: any[]): Promise<T>
      off(eventName: string, handle: any): void
      updater: {
        progress(handle: (_progress: ProgressInfo) => void): void
        available(handle: (_info: UpdateInfo) => void): void
        downloaded(handle: (_info: UpdateDownloadedEvent) => void): void
        checking(handle: (dateStartedChecking: string) => void): void
      }
    }
    license: Unlock
  }
}
