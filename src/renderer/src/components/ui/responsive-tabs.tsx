'use client'

import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetTrigger } from '@/components/ui/sheet'
import { cn, sn } from '@/lib/utils'
import { motion } from 'framer-motion'
import { ChevronDown } from 'lucide-react'
import React, { createContext, HTMLProps, useContext, useEffect, useRef, useState } from 'react'

type Orientation = 'horizontal' | 'vertical'
type IndicatorPosition = 'left' | 'right' | 'top' | 'bottom'

// Context for managing active tab state, viewport size, and orientation
const TabContext = createContext<{
  activeTab: string
  setActiveTab: (tab: string) => void
  tabRefs: React.MutableRefObject<{ [key: string]: HTMLDivElement | null }>
  isMobile: boolean
  orientation: Orientation
  indicatorPosition: IndicatorPosition
}>({
  activeTab: '',
  setActiveTab: () => {},
  tabRefs: { current: {} },
  isMobile: false,
  orientation: 'horizontal',
  indicatorPosition: 'bottom'
})
interface TabNavbarProps extends HTMLProps<HTMLDivElement> {
  children: React.ReactNode
  defaultTab: string
  onValueChange?: (value: string) => void
  orientation?: Orientation
  indicatorPosition?: IndicatorPosition
}
// Main container component
function TabNavbar({
  children,
  defaultTab,
  orientation = 'horizontal',
  indicatorPosition = 'bottom',
  className,
  ...props
}: TabNavbarProps) {
  const [activeTab, setTab] = useState(defaultTab)
  const [isMobile, setIsMobile] = useState(false)
  const tabRefs = useRef<{ [key: string]: HTMLDivElement | null }>({})
  const setActiveTab = (value: string) => {
    setTab(value)
    props.onValueChange?.(value)
  }
  useEffect(() => {
    const handleResize = () => {
      setIsMobile(window.innerWidth < 768) // 768px is the typical md breakpoint
    }

    // Set initial state
    handleResize()

    // Add event listener
    window.addEventListener('resize', handleResize)

    // Clean up
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  const childrenArray = React.Children.toArray(children)
  const activeTabLabel =
    (
      childrenArray.find(
        (child) => React.isValidElement(child) && child.props.value === activeTab
      ) as React.ReactElement
    )?.props.children || activeTab

  const contextValue = {
    activeTab,
    setActiveTab,
    tabRefs,
    isMobile,
    orientation: isMobile ? 'vertical' : orientation,
    indicatorPosition: isMobile && orientation === 'horizontal' ? 'left' : indicatorPosition
  }
  const isVertical = orientation === 'vertical'
  return (
    <TabContext.Provider value={contextValue}>
      <nav
        className={cn(
          `relative flex select-none ${isMobile ? 'items-stretch' : isVertical ? 'flex-col items-stretch pb-10' : 'items-center'} border-border ${isMobile && !isVertical ? '' : isVertical ? 'border-r h-full pr-2 mr-2' : 'border-b'}`,
          className
        )}
      >
        {isMobile && orientation === 'horizontal' ? (
          <Sheet>
            <SheetTrigger asChild>
              <Button variant="ghost" className="w-full justify-between">
                <span>{activeTabLabel}</span>
                <ChevronDown className="h-4 w-4 opacity-50" />
              </Button>
            </SheetTrigger>
            <SheetContent side="bottom" className="h-[300px]">
              <div className="flex flex-col space-y-4 pt-4">
                {React.Children.map(children, (child) => {
                  if (React.isValidElement(child)) {
                    return React.cloneElement(child, { className: 'w-full text-left py-2' } as any)
                  }
                  return child
                })}
              </div>
            </SheetContent>
          </Sheet>
        ) : (
          <>
            <div
              className={`flex ${orientation === 'vertical' ? 'flex-col space-y-1 truncate flex-auto' : 'items-center space-x-1'}`}
            >
              {children}
            </div>
            <ActiveTabIndicator />
          </>
        )}
      </nav>
    </TabContext.Provider>
  )
}

// Individual tab component
function Tab({
  children,
  value,
  className = ''
}: {
  children: React.ReactNode
  value: string
  className?: string
}) {
  const { activeTab, setActiveTab, tabRefs, orientation } = useContext(TabContext)

  return (
    <div
      ref={(el) => (tabRefs.current[value] = el)}
      className={`relative cursor-pointer px-3 py-2 text-sm font-medium transition-colors duration-200 ${activeTab === value ? 'text-foreground' : 'text-muted-foreground hover:text-foreground'} ${orientation === 'vertical' ? 'w-full text-left' : ''} ${className}`}
      onClick={() => setActiveTab(value)}
    >
      {children}
    </div>
  )
}

function ActiveTabIndicator() {
  const { activeTab, tabRefs, orientation, indicatorPosition } = useContext(TabContext)
  const [dimensions, setDimensions] = useState({ width: 0, height: 0, left: 0, top: 0 })
  const [isInitialRender, setIsInitialRender] = useState(true)

  useEffect(() => {
    const updateDimensions = () => {
      const activeTabElement = tabRefs.current[activeTab]
      if (activeTabElement) {
        const { width, height, left, top } = activeTabElement.getBoundingClientRect()
        const parentRect = activeTabElement.offsetParent?.getBoundingClientRect()
        const parentLeft = parentRect?.left || 0
        const parentTop = parentRect?.top || 0
        setDimensions({
          width,
          height,
          left: left - parentLeft,
          top: top - parentTop
        })
      }
    }

    updateDimensions()
    setIsInitialRender(false)

    window.addEventListener('resize', updateDimensions)
    return () => window.removeEventListener('resize', updateDimensions)
  }, [activeTab, tabRefs, orientation, indicatorPosition])

  const getIndicatorStyles = () => {
    switch (indicatorPosition) {
      case 'left':
        return { width: '2px', height: dimensions.height, left: 0, top: dimensions.top }
      case 'right':
        return { width: '2px', height: dimensions.height, right: 0, top: dimensions.top }
      case 'top':
        return { width: dimensions.width, height: '2px', left: dimensions.left, top: 0 }
      case 'bottom':
      default:
        return { width: dimensions.width, height: '2px', left: dimensions.left, bottom: 0 }
    }
  }

  return (
    !isInitialRender && (
      <>
        <motion.div
          className="bg-primary absolute rounded-full"
          initial={false}
          animate={getIndicatorStyles()}
          transition={{
            type: 'spring',
            stiffness: 500,
            damping: 30
          }}
        />
        {orientation === 'vertical' && (
          <motion.div
            className="bg-gradient-to-r from-primary/0 to-primary/40 blur absolute rounded-full"
            style={{
              [indicatorPosition !== 'right' ? 'marginLeft' : 'marginRight']:
                '-' + getIndicatorStyles().width
            }}
            initial={false}
            animate={sn(getIndicatorStyles(), { width: 20, borderRadius: 0 }) as any}
            transition={{
              type: 'spring',
              stiffness: 500,
              damping: 30
            }}
          />
        )}
      </>
    )
  )
}

// Export individual components for flexibility
export { Tab, TabNavbar }
