import { ThemeProvider } from "@renderer/components/app/theme-provider";
import { AppContextProvider } from "@renderer/components/pages/app-context";
import { LogsContextProvider } from "@renderer/components/pages/logs-context";
import { SettingsContextProvider } from "@renderer/components/pages/settings/context";
import SettingsDialogWrapper from "@renderer/components/pages/settings/dialog";
import { YTDLContextProvider } from "@renderer/components/pages/ytdl-context";
import YTLDPObserver from "@renderer/components/pages/ytdlp-worker";
import { Toaster } from "@renderer/components/ui/sonner";
import { TooltipProvider } from "@renderer/components/ui/tooltip";
import { client, trpc } from "@renderer/lib/api/trpc-link";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Provider as JotaiProvider } from "jotai";
import { PropsWithChildren, Suspense, useState } from "react";
import SuspenseLoader from "./components/ui/suspense-loader";
export default function AppProviders({ children }: PropsWithChildren) {
	const [queryClient] = useState(() => new QueryClient());
	const [trpcClient] = useState(() => client);
	return (
		<>
			<ThemeProvider attribute='class' defaultTheme='system' enableColorScheme disableTransitionOnChange enableSystem>
				<TooltipProvider>
					<Suspense fallback={<SuspenseLoader />}>
						<trpc.Provider client={trpcClient} queryClient={queryClient as any}>
							<QueryClientProvider client={queryClient}>
								<JotaiProvider>
									<AppContextProvider value={{ selected: null } as any}>
										<SettingsContextProvider value={{ open: false } as any}>
											<LogsContextProvider
												value={
													{
														data: [],
													} as any
												}>
												<YTDLContextProvider value={{} as any}>
													{children}
													<YTLDPObserver />
												</YTDLContextProvider>
											</LogsContextProvider>
											<SettingsDialogWrapper />
										</SettingsContextProvider>
									</AppContextProvider>
								</JotaiProvider>
							</QueryClientProvider>
						</trpc.Provider>
					</Suspense>
				</TooltipProvider>
				<Toaster />
			</ThemeProvider>
		</>
	);
}
