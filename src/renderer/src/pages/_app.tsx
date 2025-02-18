import { BaseLayoutComponent } from '@renderer/components/baseLayout'
import SuspenseLoader from '@renderer/components/ui/suspense-loader'
import AppProviders from '@renderer/providers'
import { Outlet } from 'react-router-dom'
export const Pending = () => <SuspenseLoader />

export const Catch = () => {
  return <div>Something went wrong... Caught at _app error boundary</div>
}
export default function App() {
  return (
    <>
      <AppProviders>
        <BaseLayoutComponent>
          <Outlet />
          <div id="portal"></div>
        </BaseLayoutComponent>
      </AppProviders>
    </>
  )
}
