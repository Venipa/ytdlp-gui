import { loadUrlOfWindow } from '@main/trpc/dialog.utils'
import { isProduction } from '@shared/config'
import { createEncryptedStore } from '@shared/electron/store/createYmlStore'
import { createLogger } from '@shared/logger'
import axios from 'axios'
import dayjs from 'dayjs'
import { app, BrowserWindow, dialog, ipcMain } from 'electron'
import { Conf } from 'electron-conf'
import { AppUpdater } from "electron-updater"
import EventEmitter from 'events'
import { flatten, merge, toArray } from 'lodash-es'
import { machineIdSync } from 'node-machine-id'
import os from 'os'
import { join } from 'path'
const log = createLogger('Unlock')
export default class Unlock<TWin extends BrowserWindow = BrowserWindow> {
  private mainWindow!: TWin
  private licenseWindow!: TWin
  private autoUpdater: AppUpdater
  config: any
  private store: Conf
  private fingerprint?: string
  private showAfterLoaded: boolean = true
  private events = new EventEmitter()
  get storeInstance() {
    return this.store
  }
  constructor(
    config: { api: { productId: string; key: string }; [key: string]: any },
    autoUpdater: any,
    showAfterLoaded: boolean = true
  ) {
    this.mainWindow = null as any
    this.licenseWindow = null as any
    this.autoUpdater = autoUpdater
    this.config = this.buildConfig(config)
    this.store = createEncryptedStore('unlock', { defaults: {} })
    this.showAfterLoaded = showAfterLoaded

    this.registerHandlers()
    log.debug({ config })
  }

  buildConfig(config: any) {
    return merge(
      {
        api: {
          url: 'https://api.anystack.sh/v1'
        },
        license: {
          requireEmail: false,
          checkIn: {
            value: 24,
            unit: 'hours'
          },
          encryptionKey: config.api.productId,
          trial: {
            enabled: false,
            value: 7,
            unit: 'days'
          }
        },
        updater: {
          url: 'https://dist.anystack.sh/v1/electron'
        },
        prompt: {
          title: 'Anystack',
          subtitle: 'Activate your license to get started',
          logo: 'https://anystack.sh/img/emblem.svg',
          email: 'Email address',
          licenseKey: 'License key',
          activateLicense: 'Activate license',
          trial: 'Try for 7 days',
          trialExpired:
            'Thank you for trying Anystack. Your trial has expired; to continue, please purchase a license.',
          errors: {
            NOT_FOUND: 'Your license information did not match our records.',
            SUSPENDED: 'Your license has been suspended.',
            EXPIRED: 'Your license has been expired.',
            FINGERPRINT_INVALID: 'No license was found for this device.',
            FINGERPRINT_ALREADY_EXISTS: 'An active license already exists for this device.',
            MAX_USAGE_REACHED: 'Your license has reached its activation limit.',
            RELEASE_CONSTRAINT: 'Your license has no access to this version.'
          }
        },
        confirmation: {
          title: 'You are awesome!',
          subtitle: 'Thank you for activating your product license.'
        }
      },
      config
    )
  }

  registerHandlers() {
    if (this.autoUpdater) {
      this.registerAutoUpdater()
    }

    this.registerFingerprint()
    this.registerRendererHandlers()

    setInterval(() => {
      if (this.checkinRequired()) {
        log.debug('A license check-in is required.')
        this.verifyDeviceLicense()
      }
    }, 10000)
  }

  async ifAuthorized(mainWindow?: TWin) {
    this.mainWindow = mainWindow as any

    if (this.licenseExistsOnDevice()) {
      log.debug('A license exists on this device.')

      if (this.checkinRequired()) {
        log.debug('A license check-in is required.')
        await this.verifyDeviceLicense()
      }
      this.#activated = true;
      log.debug('Opening main application.')
      return this.showAfterLoaded && this.mainWindow?.show()
    }

    log.debug('No license found on this device.')
    await this.promptLicenseWindow()
  }

  async promptLicenseWindow() {
    log.debug('Prompting license window.')

    this.licenseWindow = new BrowserWindow({
      width: 400,
      height: 480,
      maximizable: false,
      minimizable: false,
      resizable: false,
      frame: false,
      backgroundColor: '#09090B',
      alwaysOnTop: true,
      show: false,
      webPreferences: {
        nodeIntegration: true,
        contextIsolation: false,
        webSecurity: false,
        devTools: !isProduction,
        preload: join(__dirname, '../preload/index.js')
      }
    }) as TWin
    await loadUrlOfWindow(this.licenseWindow, '/license', {
      query: { data: JSON.stringify(this.config) }
    })
    this.licenseWindow.show();

    this.licenseWindow.once("close", () => {
      if (!this.#activated) app.quit();
    })
    // Open the DevTools.
    await new Promise((resolve, reject) => {
      this.events.once('activated', resolve)
      this.events.once('activation-cancelled', reject)
    })
  }

  checkinRequired() {
    const lastCheckIn: any = this.store.get('license.lastCheckIn', false)

    if (lastCheckIn === false) {
      return false
    }

    return dayjs()
      .subtract(this.config.license.checkIn.value, this.config.license.checkIn.unit)
      .isAfter(dayjs.unix(lastCheckIn))
  }

  async verifyDeviceLicense() {
    await this.validateLicense({
      key: this.store.get('license.key', false),
      scope: {
        fingerprint: this.fingerprint,
        release: {
          tag: app.getVersion()
        }
      }
    })
      .then((response) => {
        const valid = response.data.meta.valid
        const status = response.data.meta.status

        if (valid === true) {
          log.debug('License check in complete and the device license is valid.')
          this.store.set('license.lastCheckIn', dayjs().unix())
        }

        if (valid === false && status !== 'RESTRICTED') {
          log.debug('License invalid: ' + status)
          this.invalidateDeviceLicenseAndNotify(status)
        }
      })
      .catch((error) => {
        log.debug('An error occurred during the license check.')
        log.debug(error.response.data)
        this.showRequestErrorDialog(error)
      })
  }

  showRequestErrorDialog(error) {
    let errorMessage

    if (error.response.status === 422) {
      errorMessage = flatten(toArray(error.response.data.errors)).join(', ')
    } else {
      errorMessage = JSON.stringify(error.response.data)
    }

    dialog.showMessageBox({
      title: 'An unexpected error occurred',
      buttons: ['Continue'],
      type: 'warning',
      message: errorMessage
    })
  }

  invalidateDeviceLicenseAndNotify(status) {
    this.store.delete('license')

    dialog.showMessageBox({
      title: 'Your license is invalid',
      buttons: ['Continue'],
      type: 'warning',
      message: this.config.prompt.errors[status]
    })

    app.relaunch()
    app.exit()
  }

  licenseExistsOnDevice() {
    return this.store.has('license')
  }

  validateLicense(data: any) {
    return this.doRequest('validate-key', data)
  }

  activateLicense(data: any) {
    return this.doRequest(
      'activate-key',
      merge(data, {
        platform: process.platform
      })
    )
  }
  #activated: boolean = false
  registerRendererHandlers() {
    ipcMain.handle('license-config', async () => {
      return [this.config, this.store.store]
    })
    ipcMain.handle('attempt-license-activation', async (event, arg) => {
      log.debug('Attempting license activation.')
      let data = this.getLicenseValidationRequestData(arg.licenseKey, arg.email)
      return await new Promise((resolve, reject) => {
        this.validateLicense(data)
          .then((response) => {
            log.debug('Validate license request was made successfully.')

            if (response.data.meta.valid === true || response.data.meta.status === 'RESTRICTED') {
              log.debug('Restoring existing device license.')

              this.store.set('license', {
                key: arg.licenseKey,
                email: arg.email,
                fingerprint: this.fingerprint,
                lastCheckIn: dayjs().unix()
              })

              this.events.emit('activated')
              this.#activated = true;
              this.licenseWindow.close()
              this.showAfterLoaded && this.mainWindow?.show()
              resolve(['license-activated'])
            } else if (response.data.meta.status === 'FINGERPRINT_INVALID') {
              log.debug('Attempting to activate license for this device.')
              this.activateLicense({
                key: arg.licenseKey,
                fingerprint: this.fingerprint,
                name: os.hostname()
              })
                .then((response) => {
                  if (response.status === 201) {
                    log.debug('License was activated successfully.')
                    this.store.set('license', {
                      key: arg.licenseKey,
                      email: arg.email,
                      fingerprint: this.fingerprint,
                      lastCheckIn: dayjs().unix()
                    })
                    this.events.emit('activated')
                    this.#activated = true;

                    this.licenseWindow.close()
                    this.showAfterLoaded && this.mainWindow?.show()

                    return resolve(['license-activated'])
                  }
                })
                .catch((error) => {
                  log.debug('License activation has failed.')
                  if (error.response.status === 422) {
                    return reject([
                      'license-activation-failed',
                      {
                        licenseError:
                          this.config.prompt.errors[error.response.data.errors['license']],
                        emailError: error.response.data.errors['scope.contact.email']
                      }
                    ])
                  }
                })
            } else {
              log.debug('License activation has failed.')
              return reject([
                'license-activation-failed',
                {
                  licenseError: this.config.prompt.errors[response.data.meta.status]
                }
              ])
            }
          })
          .catch((error) => {
            log.debug('Validate license request resulted in an error.')
            log.debug(error.response.data)
            this.showRequestErrorDialog(error)
          })
      })
    })

    ipcMain.on('attempt-trial-run', (event, arg) => {
      if (this.store.has('trial.start') == false) {
        log.debug('Starting trial')

        this.store.set('trial', {
          start: dayjs().unix()
        })
      }

      if (this.trialExpired()) {
        log.debug('Trial has expired.')

        event.reply('trial-expired')
        return
      }

      setTimeout(() => {
        this.#activated = true;
        this.licenseWindow.close()
        this.showAfterLoaded && this.mainWindow?.show()
      }, 1000)
    })
  }

  getLicenseValidationRequestData(licenseKey, email = null) {
    let data = {
      key: licenseKey,
      scope: {
        fingerprint: this.fingerprint,
        release: {
          tag: app.getVersion()
        }
      }
    }

    if (this.config.license.requireEmail) {
      data = merge(data, {
        scope: {
          contact: {
            email: email
          }
        }
      })
    }

    return data
  }

  trialExpired() {
    const trialStart: any = this.store.get('trial.start')

    return dayjs()
      .subtract(this.config.license.trial.value, this.config.license.trial.unit)
      .isAfter(dayjs.unix(trialStart))
  }

  registerFingerprint() {
    this.fingerprint = machineIdSync()
    log.debug('Device fingerprint registered: ' + this.fingerprint)
  }

  doRequest(endpoint, data) {
    return axios.post(
      `${this.config.api.url}/products/${this.config.api.productId}/licenses/${endpoint}`,
      data,
      {
        headers: {
          Authorization: `Bearer ${this.config.api.key}`
        }
      }
    )
  }

  registerAutoUpdater() {
    const licenseKey = this.store.get('license.key', '')
    const updaterType =
      typeof this.autoUpdater.checkForUpdatesAndNotify === 'function'
        ? 'electron-builder'
        : 'electron-native'

    if (!licenseKey && this.config.license.trial.enabled == false) {
      log.debug('Skipped registration of auto updater because no license key is provided.')
      return
    }

    log.debug('Registering auto updater for ' + updaterType)

    if (updaterType === 'electron-builder') {
      this.autoUpdater.setFeedURL({
        url:
          this.config.updater.url + '/' + this.config.api.productId + '/releases?key=' + licenseKey,
        serverType: 'json',
        provider: 'generic',
        useMultipleRangeRequest: false
      } as any)

      setInterval(() => {
        this.autoUpdater.checkForUpdatesAndNotify()
      }, 1800000)
    }
    if (updaterType === 'electron-native') {
      this.autoUpdater.setFeedURL({
        url:
          this.config.updater.url +
          '/' +
          this.config.api.productId +
          '/update/' +
          process.platform +
          '/' +
          process.arch +
          '/' +
          app.getVersion() +
          '?key=' +
          licenseKey,
        serverType: 'json',
        provider: 'generic',
        useMultipleRangeRequest: false
      } as any)

      setInterval(() => {
        this.autoUpdater.checkForUpdates()
      }, 1800000)
    }

    log.debug('Checking for inital updates')
    this.autoUpdater.autoInstallOnAppQuit = true;
    this.autoUpdater.checkForUpdatesAndNotify();
  }
}
