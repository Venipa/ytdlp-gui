import { LucideHardDriveDownload } from 'lucide-react'
import AddLink from '../components/add-link'
import LinkList from '../components/link-list'

export const meta = {
  title: 'Downloads',
  icon: LucideHardDriveDownload,
  index: 0,
  show: true,
}
const Icon = meta.icon
export default function YTDLPTab() {
  return (
    <div className="grid gap-8 p-2 h-full">
      <div className="grid gap-6 pt-10">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-2">
            <Icon className="size-5" />
            <h1 className="text-lg font-semibold">Downloads</h1>
          </div>
        </div>
          <div className="flex flex-col gap-6">
            <LinkList />
            <AddLink />
          </div>
      </div>
    </div>
  )
}
