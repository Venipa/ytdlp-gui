import { electronAPI } from '@electron-toolkit/preload'
import platform from '@main/lib/platform'
import { IpcRendererEvent, contextBridge, ipcRenderer } from 'electron'
import { ELECTRON_TRPC_CHANNEL } from 'electron-trpc/main'
import { availableParallelism } from 'node:os'
import { version } from '~/package.json'
import { } from './index.d'
// Custom APIs for renderer
const api = {
  version,
  platform,
  useMica: platform.isWindows && !!process.argv.find(d => d === "--use-mica"),
  maxParallelism: availableParallelism(),
  on: (eventName: string, handle: (ev: IpcRendererEvent, ...args: any[]) => void) =>
    ipcRenderer.on(eventName, handle),
  off: (eventName: string, handle: any) => ipcRenderer.off(eventName, handle),
  invoke(actionName, ...args) {
    return ipcRenderer.invoke(actionName, ...args)
  },
  updater: {
    available(handle) {
      ipcRenderer.on('update-available', (_ev, data) => handle(data))
    },
    progress(handle) {
      ipcRenderer.on('update-download-progress', (_ev, data) => handle(data))
    },
    downloaded(handle) {
      ipcRenderer.on('update-download-done', (_ev, data) => handle(data))
    },
    checking(handle) {
      ipcRenderer.on('update-checking', (_ev, data) => handle(data))
    },
    checkForUpdates() {
      return ipcRenderer.invoke('update-check')
    }
  }
} satisfies typeof window.api

const electronTRPC = {
  sendMessage: (operation) => ipcRenderer.send(ELECTRON_TRPC_CHANNEL, operation),
  onMessage: (callback) => ipcRenderer.on(ELECTRON_TRPC_CHANNEL, (_event, args) => callback(args))
}

// Use `contextBridge` APIs to expose Electron APIs to
// renderer only if context isolation is enabled, otherwise
// just add to the DOM global.
if (process.contextIsolated) {
  try {
    contextBridge.exposeInMainWorld('electron', electronAPI)
    contextBridge.exposeInMainWorld('api', api)
    contextBridge.exposeInMainWorld('electronTRPC', electronTRPC)
  } catch (error) {
    console.error(error)
  }
} else {
  // @ts-ignore (define in dts)
  window.electron = electronAPI
  // @ts-ignore (define in dts)
  window.api = api
  // @ts-ignore (define in dts)
  window.electronTRPC = electronTRPC
}

process.on('loaded', async () => {})
