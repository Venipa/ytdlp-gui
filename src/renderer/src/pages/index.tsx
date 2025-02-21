import ClickableText from '@renderer/components/ui/clickable-text'
import { Tab, TabNavbar } from '@renderer/components/ui/responsive-tabs'
import { ScrollArea } from '@renderer/components/ui/scroll-area'
import SuspenseLoader from '@renderer/components/ui/suspense-loader'
import { cn } from '@renderer/lib/utils'
import config, { NodeEnv } from '@shared/config'
import {
  createElement,
  Fragment,
  HTMLProps,
  ReactElement,
  Suspense,
  useMemo,
  useState
} from 'react'
import { toast } from 'sonner'
import StatusBar from './components/status-bar'
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
    customLayout?: boolean
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
const getSectionMetaByTitle = (title: string) =>
  sectionValues.find((d) => d.meta.title === title)?.meta
export default function SettingsWindow() {
  const [selectedTab, setSelectedTab] = useState<string>(sectionTabs[0].title)
  const selectedContent = useMemo(
    () => (selectedTab && createElement(getSectionContentByTitle(selectedTab) as any)) || null,
    [selectedTab]
  )
  const selectedMeta = useMemo(
    () => (selectedTab && (getSectionMetaByTitle(selectedTab) as any)) || null,
    [selectedTab]
  )
  const buildInfo = useMemo(() => config.git?.shortHash && `${config.git.shortHash}`, [])
  const appVersion = useMemo(() => `v${window.api.version}`, [])
  const ContentLayout: typeof ScrollArea = useMemo(
    () => (selectedMeta?.customLayout ? Fragment as any : ScrollArea),
    [selectedMeta]
  )
  return (
    <div className={cn('absolute inset-0 flex flex-col px-0 h-full')}>
      <div className="flex flex-col flex-shrink-0 h-full flex-auto">
        <div className="grid grid-cols-[148px_1fr] h-full flex-auto overflow-hidden">
          <TabNavbar
            defaultTab={selectedTab}
            onValueChange={setSelectedTab}
            orientation="vertical"
            indicatorPosition="right"
            className="h-full bg-background-2 pt-16 pb-6 relative"
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
            <div className="flex-auto"></div>
            <div className="flex flex-col text-xs text-muted-foreground px-2 items-end flex-shrink-0">
              <div>{config.appInfo.name}</div>
              <ClickableText
                onClick={() =>
                  navigator.clipboard
                    .writeText(appVersion + ((buildInfo && ` b${buildInfo}`) || ''))
                    .then(() => toast('Copied app version'))
                }
              >
                {appVersion}
              </ClickableText>
              {buildInfo && (
                <ClickableText
                  onClick={() =>
                    navigator.clipboard
                      .writeText(buildInfo)
                      .then(() => toast('Copied build identifier'))
                  }
                >
                  {buildInfo}
                </ClickableText>
              )}
              <span>{NodeEnv}</span>
            </div>
          </TabNavbar>
          <ContentLayout
            className="relative h-full px-6"

          >
            {selectedContent ? (
              <Suspense fallback={<SuspenseLoader />}>{selectedContent}</Suspense>
            ) : (
              <div className="flex flex-col items-center justify-center h-20">Nothing here ?.?</div>
            )}
          </ContentLayout>
        </div>
        <StatusBar />
      </div>
    </div>
  )
}
