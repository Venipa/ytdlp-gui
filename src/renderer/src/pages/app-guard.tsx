import { SuspenseLoaderOptions } from "@renderer/components/ui/suspense-loader";
import { RotateWords } from "@renderer/components/ui/text-rotate";
import { tempStorage } from "@renderer/lib/atom";
import { trpc } from "@renderer/lib/trpc-link";
import { logger } from "@shared/logger";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { useEffect } from "react";
import { useResetSelectedTab } from "./index.store";

const appInitialized = atomWithStorage("appInitialized", false, tempStorage, { getOnInit: true });
const useAppInitialized = () => useAtom(appInitialized);
export default function AppGuard({ children }: { children: React.ReactNode }) {
	const [appInitialized, setAppInitialized] = useAppInitialized();
	const resetSelectedTab = useResetSelectedTab();
	const { mutateAsync: initialize, isLoading } = trpc.internals.initializeApp.useMutation({
		onError: (error) => {
			logger.error("Failed to initialize", error);
		},
	});
	useEffect(() => {
		if (appInitialized) return;
		initialize()
			.then(() => {
				setAppInitialized(true);
				resetSelectedTab();
			})
			.catch((err) => {
				if (err.message === "App already initialized") setAppInitialized(true);
			});
	}, [initialize]);
	if (appInitialized) return <>{children}</>;

	return (
		<>
			{isLoading ? (
				<SuspenseLoaderOptions
					content={<RotateWords words={["Initializing app and ytdlp...", "Checking for updates...", "Moving full speed ahead...", "Almost there..."]} delay={2000} />}
				/>
			) : (
				children
			)}
		</>
	);
}
