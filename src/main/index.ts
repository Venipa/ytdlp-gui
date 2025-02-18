import { electronApp, optimizer, platform } from '@electron-toolkit/utils'
import { isDevelopmentOrDebug, isProduction } from '@shared/config'
import { Logger, logger } from '@shared/logger'
import { BrowserWindow, Menu, MenuItem, Tray, app, shell, systemPreferences } from 'electron'
import { menubar } from 'menubar'
import { join } from 'path'
// @ts-ignore
import iconWin from '~/resources/icon.ico?asset'
// @ts-ignore
import icon from '~/resources/icon-24x24.png?asset'
// @ts-ignore
import { autoUpdater } from 'electron-updater'
import trayIconAsset from '~/resources/menuIcon_16.png?asset'
// @ts-ignore
import builderConfig from '../../electron-builder.yml'
import { attachAutoUpdaterIPC, checkLicense } from './license'
import { trpcIpcHandler } from './trpc'
import { createUrlOfPage, loadUrlOfWindow } from './trpc/dialog.utils'
import { pushWindowState } from './trpc/window.api'
if (isDevelopmentOrDebug) Logger.enableProductionMode()
const log = new Logger('App')
const trayIcon = !platform.isWindows ? (platform.isMacOS ? trayIconAsset : iconWin) : iconWin

if (platform.isMacOS && !systemPreferences.isTrustedAccessibilityClient(true)) {
  logger.warn('Missing trusted accessibility access, requesting...')
}
async function createWindow() {
  await checkLicense()
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
  const mb = menubar({
    icon: trayIcon,
    showDockIcon: false,
    preloadWindow: true,
    tray,
    showOnRightClick: false,
    index: mbIndex.path,
    loadUrlOptions: mbIndex.options as any,
    browserWindow: {
      width: 600,
      height: 600,
      minHeight: 600,
      minWidth: 600,
      movable: false,
      resizable: false,
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
  mb.on('ready', async () => {
    if (isDevelopmentOrDebug) mainWindow.webContents.openDevTools({ mode: 'detach' })
    app.setAccessibilitySupportEnabled(true)
  })
  mb.tray.setImage(trayIcon)

  // HMR for renderer base on electron-vite cli.
  // Load the remote URL for development or the local html file for production.
  await loadUrlOfWindow(mainWindow, '/')
  attachAutoUpdaterIPC(mainWindow)
  return mb
}

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.whenReady().then(() => {
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

  createWindow()

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
