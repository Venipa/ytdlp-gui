export interface AppStore {
  ytdlp: {
    path: string
    version: string
    checkForUpdate: boolean
    useGlobal: boolean
    flags: {
      nomtime: boolean
      custom: string
    }
  }
  download: { paths: string[]; selected: string }
  features: {
    clipboardMonitor: boolean,
    clipboardMonitorAutoAdd: boolean
    concurrentDownloads: number
  }
  startMinimized: boolean
  startOnBoot: boolean
  beta: boolean
}
