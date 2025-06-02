import { BaseLayoutComponent } from "@renderer/components/baseLayout";
import { Button } from "@renderer/components/ui/button";
import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import AppProviders from "@renderer/providers";
import { AlertTriangleIcon, Bug, RefreshCcw } from "lucide-react";
import { Outlet } from "react-router-dom";
import AppGuard from "./app-guard";
export const Pending = () => <SuspenseLoader />;
export const Catch = () => {
	return (
		<div className='flex flex-col items-center justify-center h-full gap-4'>
			<AlertTriangleIcon className='size-10 text-destructive' />
			<span>Something went wrong... Caught at _app error boundary</span>
			<div className='flex items-center gap-2'>
				<Button onClick={() => window.location.reload()} variant={"outline"}>
					<RefreshCcw />
					<span>Reload</span>
				</Button>
				<Button
					onClick={() => {
						window.open(
							`https://github.com/${import.meta.env.VITE_GITHUB_REPOSITORY}/issues/new?assignees=&labels=bug&projects=&template=bug_report.yml&title=%5BBUG%5D%3A+`,
							"_blank",
						);
						window.close();
					}}
					variant={"destructive"}>
					<Bug />
					<span>Report bug</span>
				</Button>
			</div>
		</div>
	);
};
export default function App() {
	return (
		<>
			<AppProviders>
				<BaseLayoutComponent>
					<AppGuard>
						<Outlet />
					</AppGuard>
					<div id='portal'></div>
				</BaseLayoutComponent>
			</AppProviders>
		</>
	);
}
