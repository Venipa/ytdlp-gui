import { is } from '@electron-toolkit/utils'
import { trpcIpcHandler } from '@main/trpc'
import {
  BrowserWindow,
  BrowserWindowConstructorOptions,
  Event,
  LoadFileOptions,
  Rectangle,
  shell
} from 'electron'
import { join } from 'path'
// @ts-ignore
import { logger } from '@shared/logger'
import { buildUrl } from 'build-url-ts'
// @ts-ignore
// @ts-ignore
import iconWin from '~/resources/icon.ico?asset'
// @ts-ignore
import icon from '~/resources/icon.png?asset'

export const createChildWindow = (options?: BrowserWindowConstructorOptions) => {
  const { parent = null } = options as BrowserWindowConstructorOptions
  const mainWindow = new BrowserWindow({
    width: 1280,
    height: 720,
    minHeight: 600,
    minWidth: 1080,
    show: false,
    backgroundColor: '#101010',
    autoHideMenuBar: true,
    ...(process.platform === 'linux' ? { icon } : { icon: iconWin }),
    ...(parent ? { modal: true, parent, center: true } : {}),
    ...(options ? options : {}),
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: false
    },
    frame: false
  })
  trpcIpcHandler.attachWindow(mainWindow)

  mainWindow.webContents.setWindowOpenHandler((details) => {
    shell.openExternal(details.url)
    return { action: 'deny' }
  })
  return mainWindow
}
export const loadUrlOfWindow = (
  win: BrowserWindow,
  page?: string,
  options: LoadFileOptions = {}
) => {
  const hashPath = page?.replace(/^(\#?\/?)/, '#/') || '#/'
  logger.debug({ query: options.query })
  if (is.dev && process.env['ELECTRON_RENDERER_URL']) {
    return win.loadURL(
      buildUrl(process.env['ELECTRON_RENDERER_URL'], {
        hash: hashPath.replace(/\#?/, ''),
        queryParams: options.query
      }).toString()
    )
  } else {
    const indexPath = join(__dirname, '../renderer/index.html')
    return win.loadFile(indexPath, { ...options, hash: hashPath })
  }
}
export const createUrlOfPage = (page?: string, options: LoadFileOptions = {}) => {
  const hashPath = page?.replace(/^(\#?\/?)/, '#/') || '#/'
  if (is.dev && process.env['ELECTRON_RENDERER_URL']) {
    const indexUrl = buildUrl(process.env['ELECTRON_RENDERER_URL'], {
      hash: hashPath.replace(/\#?/, ''),
      queryParams: options.query
    }).toString()
    logger.debug({ parsedUrl: indexUrl, hashPath })
    return { path: indexUrl, options }
  } else {
    const indexPath = buildUrl(new URL('../renderer/index.html', import.meta.url).toString(), {
      hash: hashPath
    })
    logger.info({ parsedUrl: indexPath, hashPath })

    return { path: indexPath, options: { ...options, hash: hashPath } as typeof options }
  }
}
export const preloadPath = (preload: string) => {
  return new URL(preload).toString()
}
export const waitForWindowPaint = (win: BrowserWindow) => {
  return () => new Promise<void>((resolve) => win.webContents.once('did-finish-load', resolve))
}
export function lockCenterWithParent(win: BrowserWindow) {
  let parentWindow: BrowserWindow
  if (
    !win.isModal() ||
    !(parentWindow = win.getParentWindow()!) ||
    parentWindow.isDestroyed() ||
    !parentWindow.isMovable()
  )
    return
  parentWindow.setMovable(false)
  const isParentOnTop = parentWindow.isAlwaysOnTop()
  if (!isParentOnTop) parentWindow.setAlwaysOnTop(true)
  let nestedParents: BrowserWindow[] = [parentWindow]
  // if (parentWindow.getParentWindow()) { // todo: also move nested parents
  //   let nestedParent: BrowserWindow
  //   while ((nestedParent = parentWindow.getParentWindow()!)) {
  //     if (nestedParent && nestedParents.findIndex(w => w.id === nestedParent.id) === -1) {
  //       nestedParents.push(nestedParent)
  //     }
  //   }
  // }
  const handleMove = (_ev: Event | null, newBounds?: Rectangle) => {
    nestedParents.forEach((w) => {
      if (!newBounds || !w || !win || win.isDestroyed() || w.isDestroyed()) return
      const [childw, childh] = [newBounds.width, newBounds.height]
      const [childx, childy] = [newBounds.x, newBounds.y]
      const [width, height] = w.getSize()
      const realX = childx + childw / 2 - width / 2
      const realY = childy + childh / 2 - height / 2
      w.setPosition(realX, realY)
    })
  }
  const setChildToParentCenter = () => {
    if (!parentWindow || !win || win.isDestroyed() || parentWindow.isDestroyed()) return
    const [childw, childh] = win.getSize()
    const [width, height] = parentWindow.getSize()
    const [x, y] = parentWindow.getPosition()
    const realX = x + width / 2 - childw / 2
    const realY = y + height / 2 - childh / 2
    win.setPosition(realX, realY)
  }
  win.on('will-move', handleMove)
  win.on('will-resize', handleMove)
  win.once('show', () => {
    parentWindow.setMovable(false)
    setChildToParentCenter()
    win.focus()
  })
  win.once('close', () => {
    parentWindow.setMovable(true)
    if (!isParentOnTop) parentWindow.setAlwaysOnTop(false)
    win.off('will-move', handleMove)
    win.off('will-resize', handleMove)
  })
  return { handleMove: (newBounds: Rectangle) => handleMove(null, newBounds) }
}
export function waitForWindowClose(win: BrowserWindow) {
  return new Promise<void>((resolve) => {
    if (!win || win.isDestroyed()) return resolve()
    win.once('closed', () => resolve())
  })
}
export function closeParentIfChildClose(win: BrowserWindow) {
  const parent = win.getParentWindow()
  if (!parent) return
  waitForWindowClose(win).then(() => parent.close())
}
