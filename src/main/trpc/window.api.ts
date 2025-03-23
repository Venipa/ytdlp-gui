import { platform } from '@electron-toolkit/utils'
import { publicProcedure, router } from '@main/trpc/trpc'
import { createLogger } from '@shared/logger'
import { inferProcedureOutput } from '@trpc/server'
import { observable } from '@trpc/server/observable'
import { BrowserWindow, ipcMain, IpcMainEvent } from 'electron'
import EventEmitter from 'events'
import { pick } from 'lodash-es'
import { z } from 'zod'
const log = createLogger('windowRouter')
log.debug('init')
export const windowMain = new EventEmitter()
export const pushWindowState = () => windowMain.emit('windowState')
const getWindowState = async (window: BrowserWindow) => {
  if (window.isDestroyed()) return null;
  return {
    ...pick(window, [
      'id',
      'closable',
      'maximizable',
      'minimizable',
      'isFocused',
      'isMaximized',
      'isMinimized',
      'isVisible',
      'isDestroyed'
    ]),
    isMaximized: window.isMaximized(),
    isMinimized: window.isMinimized(),
    isVisible: window.isVisible(),
    isDestroyed: window.isDestroyed(),
    isFocused: window.isFocused(),
    parentId: window.getParentWindow()?.id,
    hasChild: !!window.getChildWindows()?.length,
    alwaysOnTop: window.isAlwaysOnTop(),
    title: window.getTitle(),
    platform
  }
}
const getState = publicProcedure.query(async ({ ctx: { window } }) => {
  return await getWindowState(window)
})
export const windowRouter = router({
  getConfig: publicProcedure.query(({ctx}) => {
    return {
      title: ctx.window.getTitle()
    }
  }),
  setConfig: publicProcedure.input(z.object({
    title: z.string()
  }).partial()).mutation(({input, ctx}) => {
    if (input.title !== undefined) ctx.window.setTitle(input.title ?? "");
    pushWindowState();

  }),
  getState,
  stateChange: publicProcedure.subscription(({ ctx }) => {
    const eventNameMap = [
      'maximize',
      'minimize',
      'closed',
      'focus',
      'hide',
      'show',
      'blur',
      'resize',
      'unmaximize',
      'ready-to-show',
      'show',
      'close'
    ]
    return observable<inferProcedureOutput<typeof getState>>((emit) => {
      const onWindowStateChange = (_ev?: IpcMainEvent, ...args: any[]) => {
        getWindowState(ctx.window).then((state) => {
          emit.next(state)
        })
      }
      eventNameMap.forEach((eventName) => ctx.window.on(eventName as any, onWindowStateChange))
      ipcMain.on('windowState', onWindowStateChange)
      windowMain.on('windowState', onWindowStateChange)
      ipcMain.setMaxListeners(100);
      return () => {
        ipcMain.on('windowState', onWindowStateChange)
        windowMain.off('windowState', onWindowStateChange)
        eventNameMap.forEach((eventName) => ctx.window.off(eventName as any, onWindowStateChange))
      }
    })
  }),
  minimize: publicProcedure.mutation(({ ctx: { window, event } }) => {
    window.minimize()
    log.child('minimize').debug(event.sender.id)
  }),
  maximize: publicProcedure.mutation(({ ctx: { window, event } }) => {
    if (window.isMaximized()) window.unmaximize()
    else window.maximize()
    log.child('maximize').debug(event.sender.id)
  }),
  close: publicProcedure.mutation(({ ctx: { window, event } }) => {
    window.close()
    log.child('close').debug(event.sender.id)
  }),
  hide: publicProcedure.mutation(({ ctx: { window, event } }) => {
    window.hide()
    log.child('hide').debug(event.sender.id)
  }),
  setSize: publicProcedure
    .input(
      z
        .object({
          width: z.number().nullish(),
          height: z.number().nullish()
        })
        .superRefine((v) => {
          if (!v.width && !v.height) throw 'Missing size param'
          return v
        })
    )
    .mutation(({ ctx: { window }, input }) => {
      const {
        height: currentHeight,
        width: currentWidth,
        x: currentX,
        y: currentY
      } = window.getContentBounds()
      const widthChanged = input.width !== undefined
      const heightChanged = input.height !== undefined
      const { width, height } = Object.assign(
        {},
        { width: currentWidth, height: currentHeight },
        input
      )
      const { x, y } = {
        x: widthChanged ? currentX + (currentWidth - width)/2 : currentX,
        y: heightChanged ? currentY : currentY
      }
      const newBounds = { width, height, x, y }
      window.setContentBounds(newBounds, true)
      window.listeners('will-resize').forEach((x) => x(null, newBounds))
      log.child('setSize').debug({ input, newBounds })
    })
})
