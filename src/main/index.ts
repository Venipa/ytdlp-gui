import { electronApp, optimizer, platform } from '@electron-toolkit/utils'
import { isDevelopmentOrDebug, isProduction } from '@shared/config'
import { Logger, logger } from '@shared/logger'
import {
  BrowserWindow,
  Menu,
  MenuItem,
  Tray,
  app,
  screen,
  shell,
  systemPreferences
} from 'electron'
import { menubar } from 'menubar'
import { join } from 'path'
// @ts-ignore
import iconWin from '~/resources/icon.ico?asset'
// @ts-ignore
import icon from '~/resources/24x24.png?asset'
// @ts-ignore
import { autoUpdater } from 'electron-updater'
import trayIconAsset from '~/resources/menuIcon_16.png?asset'
// @ts-ignore
import { clamp } from 'lodash'
// @ts-ignore
import builderConfig from '../../electron-builder.yml'
import { executableIsAvailable } from './lib/bin.utils'
import { ClipboardMonitor } from './lib/clipboardMonitor'
import { appStore } from './stores/app.store'
import { runMigrate } from './stores/queue-database'
import { trpcIpcHandler } from './trpc'
import { createUrlOfPage, loadUrlOfWindow } from './trpc/dialog.utils'
import { pushWindowState } from './trpc/window.api'
import { checkBrokenLinks, ytdl } from './trpc/ytdlp.core'
import { ytdlpEvents } from './trpc/ytdlp.ee'
import { attachAutoUpdaterIPC } from './updater'
const log = new Logger('App')
const trayIcon = !platform.isWindows ? (platform.isMacOS ? trayIconAsset : iconWin) : icon

/**
 * required for clipboard monitoring for instant download feature
 */
if (platform.isMacOS && !systemPreferences.isTrustedAccessibilityClient(true)) {
  logger.warn('Missing trusted accessibility access, requesting...')
}
async function createWindow() {
  // Create the browser window.
  const tray = new Tray(trayIcon)
  const trayMenu = new Menu()
  trayMenu.append(
    new MenuItem({ label: 'Check for updates', click: () => autoUpdater.checkForUpdates() })
  )
  trayMenu.append(new MenuItem({ type: 'separator' }))
  trayMenu.append(new MenuItem({ label: 'Quit', click: () => app.quit() }))
  tray.setIgnoreDoubleClickEvents(true)
  tray.on('click', (ev) => {
    return
  })
  tray.on('right-click', () => trayMenu.popup())
  tray.setContextMenu(trayMenu)
  const mbIndex = createUrlOfPage('/')
  const sizings = screen.getPrimaryDisplay()
  const mb = menubar({
    icon: trayIcon,
    showDockIcon: false,
    preloadWindow: true,
    tray,
    showOnRightClick: false,
    index: mbIndex.path,
    loadUrlOptions: mbIndex.options as any,
    browserWindow: {
      width: 800,
      height: 768,
      minHeight: 600,
      minWidth: 800,
      maxWidth: clamp(sizings.bounds.width, 800, clamp(1280, 800, sizings.bounds.width)),
      maxHeight: clamp(sizings.bounds.height, 600, clamp(1080, 600, sizings.bounds.height)),
      movable: false,
      resizable: true,
      maximizable: false,
      minimizable: false,
      show: false,
      backgroundColor: '#09090B',
      ...(process.platform === 'linux' ? { icon } : { icon: iconWin }),
      webPreferences: {
        preload: join(__dirname, '../preload/index.js'),
        contextIsolation: false,
        nodeIntegration: true,
        sandbox: false,
        devTools: isDevelopmentOrDebug,
        additionalArguments: [`--app-path=${__dirname}`, `--app-version=${app.getVersion()}`]
      },
      frame: false
    }
  })
  const mainWindow: BrowserWindow = await new Promise((resolve) =>
    mb.once('before-load', () => {
      trpcIpcHandler.attachWindow(mb.window!)
      resolve(mb.window!)
    })
  )
  mainWindow.setMaxListeners(100)
  mb.setMaxListeners(100)
  mainWindow.webContents.setWindowOpenHandler((details) => {
    shell.openExternal(details.url)
    return { action: 'deny' }
  })
  let isReady = false
  mb.on('ready', async () => {
    isReady = true
    if (isDevelopmentOrDebug) {
      mainWindow.webContents.openDevTools({ mode: 'detach' })
    }
    app.setAccessibilitySupportEnabled(true)
  })
  mb.tray.setImage(trayIcon)

  // HMR for renderer base on electron-vite cli.
  // Load the remote URL for development or the local html file for production.
  await loadUrlOfWindow(mainWindow, '/')
  attachAutoUpdaterIPC(mainWindow)
  if (!isProduction) {
    mainWindow.setAlwaysOnTop(true)
  }
  return mb
}
// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.whenReady().then(async () => {
  app.commandLine.appendSwitch('disable-backgrounding-occluded-windows', 'true')
  // Set app user model id for windows
  const appUserId = builderConfig.appId.split('.', 2).join('.')
  log.debug({ appUserId })
  electronApp.setAppUserModelId(appUserId)
  if (isProduction) electronApp.setAutoLaunch(true)
  if (!app.requestSingleInstanceLock()) app.quit()
  // Default open or close DevTools by F12 in development
  // and ignore CommandOrControl + R in production.
  // see https://github.com/alex8088/electron-toolkit/tree/master/packages/utils
  app.on('browser-window-created', (_, window) => {
    optimizer.watchWindowShortcuts(window)
    window.webContents.on('did-finish-load', () => {
      pushWindowState()
    })
  })
  await runMigrate()
  await checkBrokenLinks()
  ytdl.initialize() // init asynchronously

  createWindow().then((w) => {
    const isWindowFocused = () => w.window!.isFocused()
    const clipboardWatcher = new ClipboardMonitor({
      distinct: true,
      onHttpsText(value) {
        log.debug('found https link in clipboard', { value })
        if (isWindowFocused()) return
        if (appStore.store.features.clipboardMonitor) {
          if (appStore.store.features.clipboardMonitorAutoAdd) ytdlpEvents.emit('add', value)
          else ytdlpEvents.emit('autoAdd', value)
        }
      }
    })
    appStore.onDidChange('features', (features) => {
      if (features?.clipboardMonitor) clipboardWatcher.start()
      else clipboardWatcher.stop()
    })
    appStore.onDidChange('ytdlp.useGlobal', (useGlobal) => {
      if (useGlobal) {
        const newPath = executableIsAvailable('yt-dlp')
        ytdl.ytdlp.setBinaryPath(newPath ?? ytdl.currentDownloadPath)
      } else ytdl.ytdlp.setBinaryPath(ytdl.currentDownloadPath)
    })
    if (appStore.store.features?.clipboardMonitor) clipboardWatcher.start()
  })

  app.on('activate', function () {
    // On macOS it's common to re-create a window in the app when the
    // dock icon is clicked and there are no other windows open.
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

// Quit when all windows are closed, except on macOS. There, it's common
// for applications and their menu bar to stay active until the user quits
// explicitly with Cmd + Q.
app.on('window-all-closed', () => {
  if (!platform.isMacOS) {
    app.quit()
  }
})

// In this file you can include the rest of your app"s specific main process
// code. You can also put them in separate files and require them here.
