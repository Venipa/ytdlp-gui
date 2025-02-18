import AppNavBar from '@renderer/components/ui/app-navbar'
import { useWindowState } from '@renderer/lib/useWindowState'
import { cn } from '@renderer/lib/utils'
import { PropsWithChildren, useMemo } from 'react'
import { useLocation } from 'react-router-dom'
import { TooltipProvider } from './ui/tooltip'
const translucentPaths = ['/', '/onboarding', '/onboarding/completed', '/settings', '/license']
export function BaseLayoutComponent({ children }: PropsWithChildren) {
  const { pathname } = useLocation()
  const { windowState } = useWindowState()
  console.log({ pathname })
  const isTranslucentRoute = useMemo(() => translucentPaths.includes(pathname), [pathname])
  return (
    <TooltipProvider>
      <div
        className={cn(
          'flex flex-col h-full transition-transform scale-100 ease-out',
          windowState?.hasChild && 'blur-md scale-95'
        )}
      >
        <AppNavBar
          variant={isTranslucentRoute ? 'transparent' : 'default'}
          className={cn('flex-shrink-0', isTranslucentRoute && '-mb-10 z-50')}
        />
        <div className="relative flex flex-col h-full overflow-auto">{children}</div>
      </div>
    </TooltipProvider>
  )
}
