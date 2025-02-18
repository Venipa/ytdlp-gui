declare module 'ytdlp-desktop' {
  export { };
}
declare module 'ytdlp-desktop/types' {
  export type YTDLState = 'progressing' | 'done'
  export type YTDLStatus =
    | { action: string; data?: any; state: YTDLState }
    | { action: string; error: any; state: YTDLState }
  export type YTDLDownloadStatus =
    | {
        percent?: number
        totalSize?: string
        currentSpeed?: string
        eta?: string
      }
    | { percent: number; error: any }

    export interface YTDLItem {
      id: string
      state: string;
      title: string
      filesize: number
      type: string
      source: string
      url: string
      filepath: string
      retryCount: number
      error?: any
    }
}
