import { ElectronAPI } from '@electron-toolkit/preload'
import type { platform } from '@electron-toolkit/utils'
import type { ProgressInfo, UpdateDownloadedEvent, UpdateInfo } from 'electron-updater'
declare global {
  interface Window {
    electron: ElectronAPI
    api: {
      version: string
      platform: typeof platform
      useMica: boolean
      maxParallelism: number
      on(eventName: string, handle: (ev: IpcRendererEvent, ...args: any[]) => void): void
      invoke<T = any>(actionName: string, ...args: any[]): Promise<T>
      off(eventName: string, handle: any): void
      updater: {
        progress(handle: (_progress: ProgressInfo) => void): void
        available(handle: (_info: UpdateInfo) => void): void
        downloaded(handle: (_info: UpdateDownloadedEvent) => void): void
        checking(handle: (dateStartedChecking: string) => void): void
        checkForUpdates(): Promise<void>;
      }
    }
  }
}
