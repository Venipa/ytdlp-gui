import { Button } from "@renderer/components/ui/button";
import ClickableText from "@renderer/components/ui/clickable-text";
import { Progress } from "@renderer/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@renderer/components/ui/select";
import { Spinner } from "@renderer/components/ui/spinner";
import { trpc } from "@renderer/lib/trpc-link";
import prettyBytes from "pretty-bytes";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

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
	dependencyKey: string;
	loadingText?: string;
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
	if (dependency.installed?.version) return dependency.installed.version;
	if (dependency.compatibleVersions.length) return dependency.compatibleVersions[0];
	return dependency.latestVersion ?? "";
}

export function DependenciesForm({
	dependencyKey,
	loadingText = "Loading dependency...",
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
	const { data: dependencies, isLoading } = trpc.dependencies.list.useQuery();
	const [selectedVersion, setSelectedVersion] = useState<string>("");
	const [progress, setProgress] = useState<DependencyProgress | null>(null);
	const { mutateAsync: installDependency, isLoading: isInstalling } = trpc.dependencies.download.useMutation();
	const { mutateAsync: removeDependency, isLoading: isRemoving } = trpc.dependencies.removeDependency.useMutation();
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();
	trpc.dependencies.downloadProgress.useSubscription(undefined, {
		onData: (progress) => {
			if (progress.key !== dependencyKey) return;
			setProgress(progress as DependencyProgress);
			if (progress.state === "completed") {
				toast(`${dependencyKey} ${completedDownloadMessage}`);
				void utils.dependencies.list.invalidate();
			}
			if (progress.state === "error") {
				toast(`${dependencyKey} ${failedDownloadMessage}`, { description: progress.message });
			}
			if (progress.state === "cancelled") {
				toast(`${dependencyKey} ${cancelledDownloadMessage}`);
			}
		},
	});

	const dependency = useMemo(() => dependencies?.find((item) => item.key === dependencyKey) ?? null, [dependencies, dependencyKey]);

	useEffect(() => {
		if (!dependency) return;
		if (selectedVersion) return;
		setSelectedVersion(resolveDefaultVersion(dependency));
	}, [dependency, selectedVersion]);

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

	return (
		<div className='flex flex-col gap-4'>
			{isLoading && (
				<div className='flex items-center gap-2 text-sm text-muted-foreground'>
					<Spinner size='sm' />
					<span>{loadingText}</span>
				</div>
			)}
			{dependency && (
				<div key={dependency.key} className='rounded-md border border-border p-3 flex flex-col gap-3'>
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
						<Select value={selectedVersion} onValueChange={setSelectedVersion} disabled={!hasCompatibleVersions || isBusy}>
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
							onClick={() =>
								installDependency({ key: dependency.key as never, version: selectedVersion })
									.then(() => {
										toast(`${startedDownloadMessage} ${dependency.name} ${selectedVersion}`);
									})
									.catch((error: Error) => {
										toast(`${failedDownloadMessage} ${dependency.name}`, { description: error.message });
									})
							}>
							{dependency.installed?.version === selectedVersion ? reinstallLabel : dependency.installed ? updateLabel : installLabel}
						</Button>
						<Button
							size='sm'
							variant='destructive'
							disabled={!canRemove}
							onClick={() =>
								removeDependency({ key: dependency.key as never })
									.then((result) => {
										toast(`${removedMessage} ${dependency.name}`, {
											description: `Reclaimed ${prettyBytes(result.reclaimedBytes ?? 0)}`,
										});
										void utils.dependencies.list.invalidate();
										setProgress({
											key: dependency.key,
											version: dependency.installed?.version ?? selectedVersion,
											state: "idle",
											fileName: null,
											percent: 0,
											downloadedBytes: 0,
											totalBytes: null,
											message: "Dependency removed",
										});
									})
									.catch((error: Error) => {
										toast(`${failedRemoveMessage} ${dependency.name}`, { description: error.message });
									})
							}>
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
							) : dependency?.installed?.path ? (
								<>
									<ClickableText
										onClick={() => {
											if (dependency?.installed?.path) {
												openPath({ path: dependency.installed.path, openParent: true });
											}
										}}>
										Open folder
									</ClickableText>
								</>
							) : null}
						</div>
					</div>
				</div>
			)}
		</div>
	);
}
