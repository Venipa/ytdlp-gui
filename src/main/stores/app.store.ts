import { createEncryptedStore } from '@shared/electron/store/createYmlStore'
import { PathsOf } from '@shared/electron/store/inferKey'
export interface AppLicense {
  code: string
  expires: string
}
export interface AppStore {
  active: AppLicense | null | undefined
  licenses: AppLicense[]
}
const store = createEncryptedStore<AppStore>('app-settings', {
  defaults: {
    licenses: [],
    active: null
  }
})

export type AppStoreKeys = PathsOf<AppStore, true>

export { store as appStore }
