import { BrowserWindow, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
export function attachAutoUpdaterIPC(win: BrowserWindow) {
  autoUpdater.on('update-available', (info) => win.webContents.send('update-available', info))
  autoUpdater.on('update-not-available', (info) => {
    win.webContents.send('update-available', false)
    win.webContents.send('update-checking', false)
  })
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
