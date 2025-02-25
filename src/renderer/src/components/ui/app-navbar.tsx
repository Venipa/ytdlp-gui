import {
  WindowControlBar,
  WindowsControlBarProps
} from '@renderer/components/ui/window/control-bar'
import { useWindowConfig, useWindowControls, useWindowState } from '@renderer/lib/useWindowState'
import { cn } from '@renderer/lib/utils'
import { useMemo, useRef } from 'react'
import { useLocation } from 'react-router-dom'
import { useEventListener } from 'usehooks-ts'
import { ThemeToggle } from '../app/theme-toggle'
import { Separator } from './separator'
import { Spinner } from './spinner'
import { QTooltip, TooltipProvider } from './tooltip'
const quitPaths: string[] = []
export default function AppNavBar({
  className,
  ...props
}: { className?: string } & WindowsControlBarProps) {
  const windowRef = useRef(document)
  const [configState, setWindowConfig] = useWindowConfig()
  const { pathname } = useLocation()
  const isQuitRoute = useMemo(() => quitPaths.includes(pathname), [pathname])
  const { windowState } = useWindowState()
  const { hide: onClose, close, maximize: onMaximize, minimize: onMinimize } = useWindowControls()
  useEventListener(
    'keydown',
    (ev) => {
      if (ev.key === 'Escape' && windowState?.parentId) close()
      else if (ev.key === 'Escape') onClose()
    },
    windowRef,
    { passive: true }
  )
  return (
    <TooltipProvider delayDuration={100}>
      <WindowControlBar
        title={configState?.title || <Spinner />}
        state={{ ...windowState, title: '' } as any}
        className={cn('h-10 px-1.5', className)}
        {...{ onMinimize, onMaximize, onClose: isQuitRoute ? close : onClose }}
        {...props}
      >
        {windowState && (
          <>
            <div className="flex items-center space-x-2 mr-2">
              <QTooltip content="Theme" align="center">
                <ThemeToggle size="sm" />
              </QTooltip>
              {(props.variant !== 'transparent' && (
                <Separator orientation="vertical" className="h-9" />
              )) || <div></div>}
            </div>
          </>
        )}
      </WindowControlBar>
    </TooltipProvider>
  )
}
