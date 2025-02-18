import { AppStore } from '@main/stores/AppStore'
import { trpc } from '@renderer/lib/trpc-link'
import { Context, createContext, Provider, useContext, useMemo } from 'react'
type AppContext = {
  settings: AppStore & Record<string, any>
  setSetting<T = any>(key: string, value: any): Promise<T>
  getSetting<T = any>(key?: string): Promise<T>
}
type AppContextType = Context<AppContext>
const appContext: AppContextType = createContext({} as any)
const useApp = () => useContext(appContext)

const AppContextProvider: Provider<AppContext> = (({ value, ...props }) => {
  const utils = trpc.useUtils()
  const [settings] = trpc.settings.index.useSuspenseQuery(undefined)
  trpc.settings.onChange.useSubscription(undefined, {
    onData(data) {
      console.log("settings", {newData: data})
      utils.settings.index.setData(undefined, data)
    }
  })
  const getSetting = useMemo(
    () => (key?: string) => (!key ? utils.internals.getAll.fetch() : utils.settings.key.fetch(key)),
    [utils.internals.get]
  )
  const { mutateAsync: _setSetting } = trpc.settings.update.useMutation()
  const setSetting = useMemo(
    () => (key: string, value: any) => _setSetting({ key, value }) as Promise<any>,
    []
  )

  return (
    <appContext.Provider
      value={{ getSetting, setSetting, settings }}
      {...props}
    ></appContext.Provider>
  )
}) as any
export { AppContextProvider, useApp }
