import { CatIcon } from "lucide-react"

export default function NotFound() {
  return <div className="flex-center h-full gap-4 select-none">
    <CatIcon className="fill-pink-500/70 stroke-pink-200 stroke-1 size-24" />
    <div className="text-lg font-semibold text-muted-foreground">Page not found</div>
  </div>
}
