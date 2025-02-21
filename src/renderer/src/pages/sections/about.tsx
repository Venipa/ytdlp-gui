import ClickableText from '@renderer/components/ui/clickable-text'
import { QTooltip } from '@renderer/components/ui/tooltip'
import config, { NodeEnv } from '@shared/config'
import { formatDistanceToNow, isValid } from 'date-fns'
import { DotIcon } from 'lucide-react'
import { useMemo } from 'react'
import { toast } from 'sonner'
import Logo from '../../components/app/logo'

export const meta = {
  title: 'About',
  index: 99
}
export default function AboutTab() {
  const lastCommitDate = useMemo(
    () =>
      !config.git?.committer?.date ||
      !isValid(new Date(config.git?.committer?.date as any * 1000)) ||
      formatDistanceToNow(config.git?.committer?.date as any * 1000, { addSuffix: true }),
    []
  )
  const buildInfo = useMemo(
    () => `v${config.appInfo.version} ${config.git.shortHash ? `#${config.git.shortHash}` : ''} (${NodeEnv})`,
    []
  )
  return (
    <div className="p-2 pt-16 flex flex-col space-y-4">
      <div className="flex items-center space-x-6 group">
        <QTooltip content="Open website" asChild>
          <a
            href="https://ytdlpd.venipa.net"
            target="_blank"
            rel="noopener"
            className='flex-shrink-0'
          >
            <Logo className="size-12 group-hover:opacity-100 opacity-80" />
          </a>
        </QTooltip>
        <div className="flex flex-col">
          <div>About {config.title}</div>
          <div className="text-xs pt-0.5 whitespace-nowrap text-muted-foreground flex items-center flex-wrap gap-x-2">
            <ClickableText onClick={() => buildInfo && navigator.clipboard.writeText(buildInfo).then(() => toast('Copied build info'))}>
              {buildInfo}
            </ClickableText>
            {lastCommitDate && (
              <>
                <DotIcon className="size-4 -mx-2" />
                <span>{lastCommitDate}</span>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
