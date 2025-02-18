import { logger } from '@shared/logger'
import { ipcRenderer } from 'electron'

export default class Unlock {
  isReady: boolean = false
  loading: boolean = false
  email: string = null as any
  emailError: any = null
  licenseKey: string = null as any
  licenseError: any = null
  activated: boolean = false
  prompt: any
  confirmation: any
  api: any
  license: any
  licenseData: any
  logo: any
  initialized = false
  async init() {
    if (this.initialized) return
    const params = new URLSearchParams(window.location.search)
    let data = JSON.parse(params.get('data')!)
    let missingDataFromQuery = !data
    let currentActiveLicense: any = null
    if (missingDataFromQuery) {
      await ipcRenderer.invoke('license-config').then((req_data) => {
        const [license, licenseData] = [req_data].flat()
        if (req_data) {
          data = license
          missingDataFromQuery = !data
          currentActiveLicense = licenseData
        }
      })
    }
    logger.debug({ data, missingDataFromQuery })
    if (missingDataFromQuery) {
      this.initialized = true
      return
    }
    logger.debug({ data, params })
    this.prompt = data.prompt ?? {}
    this.confirmation = data.confirmation ?? {}
    this.api = data.api ?? {}
    this.license = data.license ?? {}
    this.logo = data.logo
    this.licenseData = currentActiveLicense
    this.activated = !!currentActiveLicense?.license

    ipcRenderer.on('license-activation-failed', (event, arg) => {
      this.loading = false
      this.licenseError = arg.licenseError
      this.emailError = arg.emailError
    })

    ipcRenderer.on('trial-expired', (event, arg) => {
      alert(this.prompt.trialExpired)
    })
    ipcRenderer.on('license-config-update', (ev, req_data) => {
      const [_license, licenseData] = [req_data].flat()
      if (req_data) {
        this.licenseData = licenseData
        this.activated = !!licenseData?.license
      }
    })
    ipcRenderer.on('license-activated', (event, arg) => {
      this.activated = true
    })
    this.initialized = true
    this.isReady = true
  }

  activateLicense({ email, license }: { email?: string; license: string }) {
    this.loading = true
    this.emailError = null
    this.licenseError = null
    this.licenseKey = license
    this.email = email as any
    return ipcRenderer
      .invoke('attempt-license-activation', {
        licenseKey: this.licenseKey,
        email: this.email
      })
      .then((data) => {
        const [code] = [data].flat()
        if (code === 'license-activated') {
          setTimeout(() => {
            this.activated = true
          }, 500)
        }
      })
      .catch((err) => {
        const [errorCode, data] = [err].flat()
        if (errorCode === 'license-activation-failed') {
          this.loading = false
          this.licenseError = data.licenseError
          this.emailError = data.emailError
        }
        return Promise.reject([errorCode, data])
      })
  }

  startTrial() {
    ipcRenderer.send('attempt-trial-run')
  }
}
