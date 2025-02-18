import ClickableText from '@renderer/components/ui/clickable-text'
import { Tab, TabNavbar } from '@renderer/components/ui/responsive-tabs'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { Slider } from '@renderer/components/ui/slider'
import SuspenseLoader from '@renderer/components/ui/suspense-loader'
import { cn } from '@renderer/lib/utils'
import config, { NodeEnv } from '@shared/config'
import { Volume2Icon } from 'lucide-react'
import { createElement, HTMLProps, ReactElement, Suspense, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useDebounceCallback } from 'usehooks-ts'
import { useTone } from './components/tone-context'
type Element = <T extends HTMLProps<HTMLDivElement> = HTMLProps<HTMLDivElement>>(
  props?: T
) => ReactElement<T, any>
type Module = {
  default: Element
  meta: {
    title: string
    icon?: Element
    index?: number
    show?: boolean
  }
}
const SECTIONTABS = import.meta.glob<Module>(`./sections/*.tsx`, { eager: true })
const sectionValues = Object.values(SECTIONTABS).filter((d) => d.default)
const sectionTabs = sectionValues
  .map(({ meta }, i) => ({ ...meta, index: meta.index !== undefined ? meta.index : i }))
  .filter((d) => d.show !== false)
  .sort((a, b) => a.index - b.index)
const getSectionContentByTitle = (title: string) =>
  sectionValues.find((d) => d.meta.title === title)?.default
export default function SettingsWindow() {
  const [selectedTab, setSelectedTab] = useState<string>(sectionTabs[0].title)
  const selectedContent = useMemo(
    () => (selectedTab && createElement(getSectionContentByTitle(selectedTab) as any)) || null,
    [selectedTab]
  )
  const { selected: tone, setVolume } = useTone()
  const buildInfo = useMemo(() => config.git?.shortHash && `${config.git.shortHash}`, [])
  const appVersion = useMemo(() => `v${window.api.version}`, [])
  const handleVolumeChange = useDebounceCallback(([newVolume]: [number]) => {
    setVolume(newVolume)
  }, 100)
  return (
    <div className={cn('absolute inset-0 flex flex-col px-0 h-full')}>
      <div className="grid grid-cols-[148px_1fr] flex-shrink-0 h-full flex-auto -mt-6">
        <TabNavbar
          defaultTab={selectedTab}
          onValueChange={setSelectedTab}
          orientation="vertical"
          indicatorPosition="right"
          className="h-full bg-background-2 pt-24 pb-6 relative"
        >
          {sectionTabs.map(({ title, icon: Icon }) => {
            return (
              <Tab value={title!} key={title}>
                <div className="flex items-center flex-row-reverse gap-x-2">
                  {Icon && <Icon className="size-4" />}
                  <div>{title}</div>
                </div>
              </Tab>
            )
          })}
          <div className="flex-auto flex flex-col items-end justify-end group">
            <div className="flex flex-col gap-4 justify-end mr-2 mb-2 py-4">
              <Slider
                min={0}
                max={100}
                step={1}
                defaultValue={[tone?.volume ?? 15]}
                onValueChange={handleVolumeChange}
                orientation={'vertical'}
                className="h-[100px] opacity-0 group-hover:opacity-100"
              ></Slider>
              <Volume2Icon className="text-muted-foreground size-5 opacity-50 group-hover:opacity-100" />
            </div>
          </div>

          <div className="flex flex-col text-xs text-muted-foreground px-2 items-end">
            <div>{config.appInfo.name}</div>
            <ClickableText
              onClick={() =>
                navigator.clipboard
                  .writeText(appVersion + (buildInfo && ` b${buildInfo}` || ''))
                  .then(() => toast('Copied app version'))
              }
            >
              {appVersion}
            </ClickableText>
            {buildInfo && (
              <ClickableText
                onClick={() =>
                  navigator.clipboard.writeText(buildInfo).then(() => toast('Copied build identifier'))
                }
              >
                {buildInfo}
              </ClickableText>
            )}
            <span>{NodeEnv}</span>
          </div>
        </TabNavbar>
        <ScrollArea className="px-6 pt-16 relative">
          {selectedContent ? (
            <Suspense fallback={<SuspenseLoader />}>{selectedContent}</Suspense>
          ) : (
            <div className="flex flex-col items-center justify-center h-20">Nothing here ?.?</div>
          )}
        </ScrollArea>
      </div>
    </div>
  )
}
