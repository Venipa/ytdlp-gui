import { BrowserWindow, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import Unlock from './unlock'

export async function checkLicense(win?: BrowserWindow) {
  const unlock = new Unlock(
    {
      api: {
        productId: import.meta.env.MAIN_VITE_ANYSTACK_PRODUCT_ID,
        key: import.meta.env.MAIN_VITE_ANYSTACK_API_KEY
      },
      license: {
        requireEmail: true,
        checkIn: {
          value: 24,
          unit: 'hours'
        },
        trial: {
          enabled: true,
          value: 7,
          unit: 'days'
        }
      }
    },
    autoUpdater,
    false
  )
  await unlock.ifAuthorized(win)
  if (win) {
    const sendInfo = () =>
      win.webContents.send('license-config', [unlock.config, unlock.storeInstance.store])
    win.webContents.on('did-finish-load', sendInfo)
  }
}
export function attachAutoUpdaterIPC(win: BrowserWindow) {
  autoUpdater.on('update-available', (info) => win.webContents.send('update-available', info))
  autoUpdater.on('download-progress', (info) =>
    win.webContents.send('update-download-progress', info)
  )
  autoUpdater.on('update-downloaded', (info) => win.webContents.send('update-download-done', info))
  autoUpdater.on('checking-for-update', () =>
    win.webContents.send('update-checking', new Date().toISOString())
  )
  autoUpdater.signals.updateDownloaded(async (x) => {
    const releaseNotes = (
      typeof x.releaseNotes === 'string'
        ? x.releaseNotes
        : x.releaseNotes?.map((x) => x.note).join('\n')
    )
      ?.replace(/<[^>]+>/g, '')
      .trimStart()
    return await dialog
      .showMessageBox({
        title: `Update available (${x.version})`,
        message: `Hey there is a new version which you can update to.\n\n${
          process.platform === 'win32' ? releaseNotes : x.releaseName
        }`,
        type: 'question',
        buttons: ['Update now', 'Update on quit', 'Cancel'],
        cancelId: -1
      })
      .then(({ response }) => {
        if (response === 0) autoUpdater.quitAndInstall()
        else if (response === 1) autoUpdater.autoInstallOnAppQuit = true;
      })
  })
}
