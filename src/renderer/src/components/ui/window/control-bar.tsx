import type { RouterOutput } from '@main/api'
import { cn } from '@renderer/lib/utils'
import { cva, VariantProps } from 'class-variance-authority'
import * as React from 'react'
import { ControlButton } from './control-button'
type WindowTrpcState = RouterOutput['window']['getState']
type WindowState = WindowTrpcState

const windowControlBarVariants = cva('flex items-center justify-between border-b h-10', {
  variants: {
    variant: {
      default: 'bg-background',
      transparent: 'bg-transparent border-b-transparent'
    }
  },
  defaultVariants: {
    variant: 'default'
  }
})

export interface WindowsControlBarProps
  extends React.PropsWithChildren<VariantProps<typeof windowControlBarVariants>> {
  className?: string
  state?: NonNullable<WindowState>
  title?: React.ReactNode | string
  onMinimize?: () => void
  onMaximize?: () => void
  onClose?: () => void
}
const WindowControlBar = React.forwardRef<HTMLDivElement, WindowsControlBarProps>(
  (
    {
      children,
      className,
      title,
      variant,
      onMinimize,
      onMaximize,
      onClose,
      state = {} as NonNullable<WindowState>,
      ...props
    },
    ref
  ) => {
    return (
      <div
        ref={ref}
        className={cn(windowControlBarVariants({ className: cn(className, 'relative'), variant }))}
        {...props}
      >
        <div className="flex flex-1 items-center px-2 drag h-full space-x-2">
          <div
            className={cn(
              state.parentId && 'absolute inset-0 flex items-center justify-center',
              state.platform?.isMacOS && !state.isMaximized && 'pl-14'
            )}
          >
            <h1 className="text-sm font-medium">{state.title}</h1>
          </div>
        </div>
        {!state.parentId && children}
        <div className="flex space-x-0.5">
          {!state.alwaysOnTop && (
            <ControlButton
              disabled={!state.minimizable}
              icon="minimize"
              onClick={onMinimize}
              variant={variant as any}
            />
          )}
          {state.maximizable && (
            <ControlButton
              icon={state.isMaximized ? 'maximize-out' : 'maximize'}
              onClick={onMaximize}
              variant={variant as any}
            />
          )}
          <ControlButton
            disabled={!state.closable}
            icon="close"
            variant="close"
            onClick={onClose}
          />
        </div>
      </div>
    )
  }
)
WindowControlBar.displayName = 'WindowControlBar'

export { WindowControlBar }
