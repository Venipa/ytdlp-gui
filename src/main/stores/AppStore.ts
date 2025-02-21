export interface AppStore {
  ytdlp: {
    path: string
    version: string
    checkForUpdate: boolean
  }
  download: { paths: string[]; selected: string }
  features: {
    clipboardMonitor: boolean,
    clipboardMonitorAutoAdd: boolean
  }
  beta: boolean
}
