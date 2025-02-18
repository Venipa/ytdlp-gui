import { ScrollArea } from '@renderer/components/ui/scroll-area'
import { LucideYoutube } from 'lucide-react'
import AddLink from '../components/add-link'
import LinkList from '../components/link-list'
import SelectDownloadBox from '../components/select-download-path'

export const meta = {
  title: 'YTDLP',
  icon: LucideYoutube,
  index: 0,
  show: true,
  customLayout: true
}
const Icon = meta.icon
export default function YTDLPTab() {
  return (
    <div className="overflow-hidden">
      <ScrollArea className='h-full'>
        <div className="flex flex-col gap-6 pl-2 pr-4 pb-16 mt-16">
          <LinkList />
          <div className="grid grid-cols-[1fr_240px] justify-end place-items-end justify-items-end">
            <div></div>
            <SelectDownloadBox></SelectDownloadBox>
          </div>
          <AddLink />
        </div>
      </ScrollArea>
    </div>
  )
}
