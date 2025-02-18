import type { AppRouter } from '@main/api'
import { isProduction } from '@shared/config'
import { createTRPCReact } from '@trpc/react-query'
import { ipcLink } from 'electron-trpc/renderer'

export const trpc = createTRPCReact<AppRouter>()
export const client = trpc.createClient({
  links: [ipcLink()]
})
if (!isProduction) (window as any).trpc = client;
