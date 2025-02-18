import { trpc } from '@renderer/lib/trpc-link'
import { Context, createContext, Provider, useContext, useMemo, useState } from 'react'
import type { YTDLDownloadStatus, YTDLStatus } from 'ytdlp-desktop/types'
type YTDLContext = {
  getCurrentState: () => Promise<string>
  status: YTDLStatus[] // store states up to 100 items
  download: YTDLDownloadStatus | null | undefined
}
type YTDLContextType = Context<YTDLContext>
const ytdlContext: YTDLContextType = createContext({} as any)
const useYtdl = () => useContext(ytdlContext)
const MAX_STATUS_ITEMS = 100

const YTDLContextProvider: Provider<YTDLContext> = (({ value, ...props }) => {
  const utils = trpc.useUtils()
  const getCurrentState = useMemo(() => () => utils.ytdl.state.fetch(), [utils.ytdl.state])
  const [status, setStatus] = useState<YTDLStatus[]>([])
  trpc.ytdl.status.useSubscription(undefined, {
    onData(data) {
      setStatus((s) => {
        s = [data, ...s]
        if (s.length > MAX_STATUS_ITEMS) return s.slice(0, MAX_STATUS_ITEMS)
        return s
      })
    }
  })
  const [download, setDownloadStatus] = useState<YTDLDownloadStatus | null | undefined>()
  trpc.ytdl.onDownload.useSubscription(undefined, {
    onData(data) {
      setDownloadStatus(data as any)
    }
  })

  return (
    <ytdlContext.Provider
      value={{ ...value, getCurrentState, status, download }}
      {...props}
    ></ytdlContext.Provider>
  )
}) as any
export { useYtdl, YTDLContextProvider }
