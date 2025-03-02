import { trpc } from '@renderer/lib/trpc-link'
import { logger } from '@shared/logger'
import { Context, createContext, Provider, useContext, useMemo, useState } from 'react'
export type LogEntry = {
  date: Date
  message: string
  args: any[]
  type?: 'info' | 'warn' | 'error' | 'success'
}
type LogsContext = {
  data: LogEntry[]
  clear: () => void
}
type LogsContextType = Context<LogsContext>
const logsContext: LogsContextType = createContext({ data: [] } as any)
const useLogs = () => useContext(logsContext)

const LogsContextProvider: Provider<LogsContext> = (({ value, ...props }) => {
  const utils = trpc.useUtils()
  const [data, setData] = useState<LogEntry[]>(value.data ?? [])
  trpc.events.signal.useSubscription('log', {
    onData({ date, message, type, args }: LogEntry & { date: string }) {
      logger.debug('gui log', { date, message, args })
      setData((s) => {
        const newLogEntry = { date: new Date(date), message, args, type: type || 'info' }
        return [newLogEntry, ...s]
      })
    }
  })
  const clear = useMemo(() => () => setData([]), [setData])
  logger.child('logs').debug({ data })
  return <logsContext.Provider value={{ data, clear }} {...props}></logsContext.Provider>
}) as any
export { LogsContextProvider, useLogs }
