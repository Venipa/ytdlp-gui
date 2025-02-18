import { BrowserWindow } from 'electron'
import { createIPCHandler } from 'electron-trpc/main'
import { appRouter } from './api'

export const trpcIpcHandler = createIPCHandler({
  router: appRouter,
  windows: [],
  createContext: ({ event }) => {
    return {
      window: BrowserWindow.fromWebContents(event.sender),
      event,
      path: null as any
    } as any
  }
})
