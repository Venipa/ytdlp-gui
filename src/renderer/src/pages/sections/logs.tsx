import { isProduction } from '@shared/config'
import { cva } from 'class-variance-authority'
import { format } from 'date-fns'
import { LucideLogs } from 'lucide-react'
import { CSSProperties, memo, useMemo } from 'react'
import { AutoSizer, List, ListRowProps } from 'react-virtualized'
import { LogEntry, useLogs } from '../components/logs-context'
import PlainLayout from '../components/plain-layout'

export const meta = {
  title: 'Logs',
  icon: LucideLogs,
  index: 1,
  show: !isProduction,
  customLayout: PlainLayout
}
const Icon = meta.icon
const logTypeVariant = cva('', {
  variants: {
    variant: {
      debug: 'text-purple-500',
      success: 'text-green-500',
      warn: 'text-orange-400',
      error: 'text-destructive',
      info: 'text-blue-500'
    }
  },
  defaultVariants: {
    variant: 'info'
  }
})
const LogItemView = memo(({
  date,
  message,
  args,
  type,
  ...props
}: LogEntry & { style?: CSSProperties; key?: string }) => {
  const [day, time] = useMemo(() => [format(date, 'MMM do'), format(date, 'pp')], [date])
  const messageType = useMemo(() => (type || 'info').toLowerCase(), [type])
  return (
    <div className="grid grid-cols-[100px_48px_1fr] gap-4 items-center text-sm" {...props}>
      <div className="text-right text-muted-foreground text-xs flex flex-col">
        <span>{day}</span>
        <span>{time}</span>
      </div>
      <div className={logTypeVariant({ variant: messageType as any, className: 'justify-self-center text-xs uppercase font-semibold tracking-wide' })}>
        {messageType}
      </div>
      <div className="pr-10 line-clamp-2 text-sm">{message}</div>
    </div>
  )
})
export default function LogsTab() {
  const { data: logs } = useLogs()
  const LogItem = useMemo(
    () =>
      function ({ index, style, key }: ListRowProps) {
        const logProps = logs[index]
        return <LogItemView {...logProps} {...{ style, key }} />
      },
    [logs]
  )
  return (
    <div className="flex flex-col gap-8 pt-2 pb-6 px-8 h-full">
      <div className="grid gap-6 pt-10 flex-auto">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-2">
            <Icon className="size-5" />
            <h1 className="text-lg font-semibold">Logs</h1>
          </div>
        </div>
      </div>
      <div className="h-full border rounded-lg">
        <AutoSizer>
          {({ height, width }) => (
            <List
              {...{ height, width }}
              rowRenderer={LogItem}
              rowHeight={56}
              rowCount={logs.length}
              overscanRowCount={3}
              noContentRenderer={() => (
                <div className="flex flex-col items-center justify-center text-muted-foreground">
                  No logs yet.
                </div>
              )}
            ></List>
          )}
        </AutoSizer>
      </div>
    </div>
  )
}
