import { ThemeProvider } from '@renderer/components/app/theme-provider'
import { Toaster } from '@renderer/components/ui/sonner'
import { client, trpc } from '@renderer/lib/trpc-link'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Provider as JotaiProvider } from 'jotai'
import { PropsWithChildren, Suspense, useState } from 'react'
import SuspenseLoader from './components/ui/suspense-loader'
import { ToneContextProvider } from './pages/components/tone-context'
import WKlack from './pages/components/wklack-worker'
export default function AppProviders({ children }: PropsWithChildren) {
  const [queryClient] = useState(() => new QueryClient())
  const [trpcClient] = useState(() => client)
  return (
    <>
      <ThemeProvider
        attribute="class"
        defaultTheme="system"
        enableColorScheme
        disableTransitionOnChange
        enableSystem
      >
        <Suspense fallback={<SuspenseLoader />}>
          <trpc.Provider client={trpcClient} queryClient={queryClient as any}>
            <QueryClientProvider client={queryClient}>
              <ToneContextProvider value={{ selected: null } as any}>
                <JotaiProvider>
                  {children}
                  <WKlack />
                </JotaiProvider>
              </ToneContextProvider>
            </QueryClientProvider>
          </trpc.Provider>
        </Suspense>
        <Toaster />
      </ThemeProvider>
    </>
  )
}
