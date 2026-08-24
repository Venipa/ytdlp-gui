import type { DependencyKey } from "@main/trpc/dependencies/meta";
import { Button } from "@renderer/components/ui/button";
import ClickableText from "@renderer/components/ui/clickable-text";
import { Progress } from "@renderer/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@renderer/components/ui/select";
import { Skeleton } from "@renderer/components/ui/skeleton";
import { trpc } from "@renderer/lib/api/trpc-link";
import prettyBytes from "pretty-bytes";
import { useRef, useState } from "react";
import { toast } from "sonner";

const dependencyFormSkeleton = (
	<div className='flex flex-col gap-4'>
		<div className='rounded-md border border-border p-3 flex flex-col gap-3' role='status' aria-label='Loading dependency'>
			<div className='flex items-start justify-between gap-4'>
				<div className='flex flex-col gap-1 flex-1'>
					<Skeleton className='h-4 w-24' />
					<Skeleton className='h-3 w-3/4 max-w-md' />
				</div>
				<div className='flex flex-col gap-1 items-end'>
					<Skeleton className='h-3 w-16' />
					<Skeleton className='h-3 w-12' />
				</div>
			</div>
			<div className='flex items-center gap-2'>
				<Skeleton className='h-9 flex-1' />
				<Skeleton className='h-8 w-16' />
				<Skeleton className='h-8 w-16' />
			</div>
			<div className='flex flex-col gap-1.5'>
				<Skeleton className='h-2 w-full' />
				<Skeleton className='h-3 w-32' />
			</div>
		</div>
	</div>
);

type DependencyProgress = {
	key: string;
	version: string;
	state: "idle" | "downloading" | "completed" | "error" | "cancelled";
	fileName: string | null;
	percent: number;
	downloadedBytes: number;
	totalBytes: number | null;
	message?: string;
};

interface DependenciesFormProps {
	dependencyKey: DependencyKey;
	selectVersionPlaceholder?: string;
	installLabel?: string;
	updateLabel?: string;
	reinstallLabel?: string;
	deleteLabel?: string;
	latestLabel?: string;
	usedLabel?: string;
	notAvailableLabel?: string;
	notInstalledLabel?: string;
	installedLabelPrefix?: string;
	downloadingLabel?: string;
	startedDownloadMessage?: string;
	completedDownloadMessage?: string;
	failedDownloadMessage?: string;
	cancelledDownloadMessage?: string;
	removedMessage?: string;
	failedRemoveMessage?: string;
}

function resolveDefaultVersion(dependency: {
	installed?: { version: string } | null;
	compatibleVersions: string[];
	latestVersion: string | null;
}): string {
	if (dependency.installed?.version && dependency.compatibleVersions.includes(dependency.installed.version)) {
		return dependency.installed.version;
	}
	if (dependency.compatibleVersions.length) return dependency.compatibleVersions[0];
	return dependency.latestVersion ?? "";
}

export function DependenciesForm({
	dependencyKey,
	selectVersionPlaceholder = "Select a version",
	installLabel = "Install",
	updateLabel = "Update",
	reinstallLabel = "Reinstall",
	deleteLabel = "Delete",
	latestLabel = "Latest",
	usedLabel = "Used",
	notAvailableLabel = "n/a",
	notInstalledLabel = "Not installed",
	installedLabelPrefix = "Installed",
	downloadingLabel = "Downloading",
	startedDownloadMessage = "Started downloading",
	completedDownloadMessage = "Download completed",
	failedDownloadMessage = "Download failed",
	cancelledDownloadMessage = "Download cancelled",
	removedMessage = "Removed",
	failedRemoveMessage = "Failed to remove",
}: DependenciesFormProps) {
	const utils = trpc.useUtils();
	const { data: dependency, isLoading } = trpc.dependencies.getDependency.useQuery({ key: dependencyKey });
	const [pickedVersion, setPickedVersion] = useState<string | null>(null);
	const [progress, setProgress] = useState<DependencyProgress | null>(null);
	const { mutateAsync: installDependency, isLoading: isInstalling } = trpc.dependencies.download.useMutation();
	const { mutateAsync: removeDependency, isLoading: isRemoving } = trpc.dependencies.removeDependency.useMutation();
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();
	const toastIdRef = useRef<number | string | undefined>(undefined);

	const defaultVersion = dependency ? resolveDefaultVersion(dependency) : "";
	const selectedVersion =
		pickedVersion && dependency?.compatibleVersions.includes(pickedVersion) ? pickedVersion : defaultVersion;

	trpc.dependencies.downloadProgress.useSubscription(
		{ key: dependencyKey },
		{
			onData: (event) => {
				if (event.key !== dependencyKey) return;
				setProgress(event as DependencyProgress);
				if (event.state === "completed") {
					toast.success(completedDownloadMessage, { description: dependencyKey, id: toastIdRef.current });
					void utils.dependencies.getDependency.invalidate({ key: dependencyKey });
					toastIdRef.current = undefined;
				}
				if (event.state === "error") {
					toast.error(failedDownloadMessage, { description: `${dependencyKey} ${event.message}`, id: toastIdRef.current });
					toastIdRef.current = undefined;
				}
				if (event.state === "cancelled") {
					toast.error(cancelledDownloadMessage, { description: dependencyKey, id: toastIdRef.current });
					toastIdRef.current = undefined;
				}
			},
		},
	);

	const isMutating = isInstalling || isRemoving;
	const isBusy = !!dependency?.isDownloading || progress?.state === "downloading";
	const hasCompatibleVersions = (dependency?.compatibleVersions.length ?? 0) > 0;
	const canInstall = !!dependency && !isBusy && !isMutating && !!selectedVersion;
	const canRemove = !!dependency?.installed && !isBusy && !isMutating;
	const usedSpaceBytes = dependency?.installed?.usedSpaceBytes ?? 0;
	const progressValue = isBusy ? Math.max(1, progress?.percent ?? 0) : 0;
	const statusFileName = isBusy
		? (progress?.fileName ?? downloadingLabel)
		: dependency?.installed
			? `${installedLabelPrefix} ${dependency.installed.version}`
			: notInstalledLabel;
	const statusPercent = isBusy ? `${progress?.percent ?? 0}%` : "";
	const statusDownloadedBytes = progress?.downloadedBytes ? prettyBytes(progress.downloadedBytes) : "";
	const statusTotalBytes = progress?.totalBytes ? prettyBytes(progress.totalBytes) : "";
	const installedPath = dependency?.installed?.path;

	if (isLoading) return dependencyFormSkeleton;

	return (
		<div className='flex flex-col gap-4'>
			{dependency ? (
				<div className='rounded-md border border-border p-3 flex flex-col gap-3'>
					<div className='flex items-start justify-between gap-4'>
						<div className='flex flex-col gap-1'>
							<h3 className='text-sm font-medium'>{dependency.name}</h3>
							<p className='text-xs text-muted-foreground'>{dependency.description}</p>
						</div>
						<div className='text-right text-xs text-muted-foreground'>
							<div>
								{latestLabel}: {dependency.latestVersion ?? notAvailableLabel}
							</div>
							<div>
								{usedLabel}: {prettyBytes(usedSpaceBytes)}
							</div>
						</div>
					</div>

					<div className='flex items-center gap-2'>
						<Select value={selectedVersion} onValueChange={setPickedVersion} disabled={!hasCompatibleVersions || isBusy}>
							<SelectTrigger className='h-9'>
								<SelectValue placeholder={selectVersionPlaceholder} />
							</SelectTrigger>
							<SelectContent>
								{dependency.compatibleVersions.map((version) => (
									<SelectItem value={version} key={`${dependency.key}-${version}`}>
										{version}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
						<Button
							size='sm'
							disabled={!canInstall}
							onClick={() => {
								const toastId = toast.loading(`${startedDownloadMessage} ${dependency.name} ${selectedVersion}`, { duration: 0 });
								toastIdRef.current = toastId;
								installDependency({ key: dependencyKey, version: selectedVersion })
									.then(() => {
										toast.success(completedDownloadMessage, { description: `${dependency.name} ${selectedVersion}`, id: toastId });
									})
									.catch((error: Error) => {
										toast.error(failedDownloadMessage, { description: `${dependency.name} ${selectedVersion} ${error.message}` });
									})
									.finally(() => {
										toastIdRef.current = undefined;
									});
							}}>
							{dependency.installed?.version === selectedVersion ? reinstallLabel : dependency.installed ? updateLabel : installLabel}
						</Button>
						<Button
							size='sm'
							variant='destructive'
							disabled={!canRemove}
							onClick={() => {
								const toastId = toast.loading(`${removedMessage} ${dependency.name}`, { duration: 0 });
								toastIdRef.current = toastId;
								removeDependency({ key: dependencyKey })
									.then((result) => {
										toast.success(`${removedMessage} ${dependency.name}`, {
											description: `Reclaimed ${prettyBytes(result.reclaimedBytes ?? 0)}`,
											id: toastId,
										});
										void utils.dependencies.getDependency.invalidate({ key: dependencyKey });
										setPickedVersion(null);
										setProgress(null);
									})
									.catch((error: Error) => {
										toast.error(`${failedRemoveMessage} ${dependency.name}`, { description: error.message, id: toastId });
									})
									.finally(() => {
										toastIdRef.current = undefined;
									});
							}}>
							{deleteLabel}
						</Button>
					</div>

					<div className='flex flex-col gap-1.5'>
						<Progress value={progressValue} />
						<div className='flex items-center justify-between text-xs text-muted-foreground gap-2'>
							<div className='flex gap-2'>
								<span>{statusFileName}</span>
								{statusPercent ? <span className='tabular-nums'>{statusPercent}</span> : null}
							</div>
							{statusDownloadedBytes && statusTotalBytes ? (
								<div className='flex gap-1'>
									<span className='tabular-nums'>{statusDownloadedBytes}</span>
									<span>/</span>
									<span className='tabular-nums'>{statusTotalBytes}</span>
								</div>
							) : installedPath ? (
								<ClickableText
									onClick={() => {
										openPath({ path: installedPath, openParent: true });
									}}>
									Open folder
								</ClickableText>
							) : null}
						</div>
					</div>
				</div>
			) : null}
		</div>
	);
}
