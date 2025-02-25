'use client'

import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area'
import * as React from 'react'

import { cn } from '@renderer/lib/utils'
import { logger } from '@shared/logger'
import { motion, useScroll, useTransform } from 'motion/react'

const ScrollArea = React.forwardRef<
  React.ElementRef<typeof ScrollAreaPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root> & { plain?: boolean }
>(({ className, children, plain, ...props }, ref) => {
  const scrollRef = React.useRef<HTMLDivElement>()
  const { scrollY } = useScroll({
    container: scrollRef as any,
    layoutEffect: true,
    axis: 'y'
  })
  const scrollHeight = React.useMemo(() => scrollRef.current?.scrollHeight ?? 50, [scrollRef])
  const showStartScrollBlur = useTransform(scrollY, [0, 50], [0, 1])
  const showEndScrollBlur = useTransform(scrollY, [0, scrollHeight - 50, scrollHeight], [1, 1, 0])
  logger.debug('scroller', { showEndScrollBlur: showEndScrollBlur.get(), scrollHeight })
  return (
    <ScrollAreaPrimitive.Root
      ref={ref}
      className={cn('relative overflow-hidden', className)}
      {...props}
    >
      {!plain && (
        <motion.div
          className={cn(
            'absolute top-0 inset-x-0 backdrop-blur-sm h-12 pointer-events-auto mask-to-t z-10'
          )}
          style={{ opacity: showStartScrollBlur }}
        ></motion.div>
      )}
      <ScrollAreaPrimitive.Viewport
        className="h-full w-full rounded-[inherit] pb-6"
        ref={scrollRef as any}
      >
        {children}
      </ScrollAreaPrimitive.Viewport>

      {!plain && (
        <motion.div
          className={cn(
            'absolute bottom-0 inset-x-0 backdrop-blur-sm h-8 pointer-events-auto mask-to-b z-10'
          )}
          style={{ opacity: showEndScrollBlur }}
        ></motion.div>
      )}
      <ScrollBar />
      <ScrollAreaPrimitive.Corner />
    </ScrollAreaPrimitive.Root>
  )
})
ScrollArea.displayName = ScrollAreaPrimitive.Root.displayName

const ScrollBar = React.forwardRef<
  React.ElementRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>,
  React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>
>(({ className, orientation = 'vertical', ...props }, ref) => (
  <ScrollAreaPrimitive.ScrollAreaScrollbar
    ref={ref}
    orientation={orientation}
    className={cn(
      'flex touch-none select-none transition-colors',
      orientation === 'vertical' && 'h-full w-2.5 border-l border-l-transparent p-[1px]',
      orientation === 'horizontal' && 'h-2.5 flex-col border-t border-t-transparent p-[1px]',
      className
    )}
    {...props}
  >
    <ScrollAreaPrimitive.ScrollAreaThumb className="relative flex-1 rounded-full bg-border" />
  </ScrollAreaPrimitive.ScrollAreaScrollbar>
))
ScrollBar.displayName = ScrollAreaPrimitive.ScrollAreaScrollbar.displayName

export { ScrollArea, ScrollBar }
