import { createYmlStore } from '@shared/electron/store/createYmlStore'
import { PathsOf } from '@shared/electron/store/inferKey'
import { app } from 'electron'
import { AppStore } from './AppStore'
export interface AppLicense {
  code: string
  expires: string
}
const defaultDownloadsPath = app.getPath('downloads')
const store = createYmlStore<AppStore>('app-settings', {
  defaults: {
    ytdlp: { checkForUpdate: true, flags: { nomtime: true } } as AppStore['ytdlp'],
    download: {
      paths: [defaultDownloadsPath],
      selected: defaultDownloadsPath
    },
    features: {
      clipboardMonitor: true,
      clipboardMonitorAutoAdd: true
    },
    beta: false
  }
})

export type AppStoreKeys = PathsOf<AppStore, true>

export { store as appStore }
