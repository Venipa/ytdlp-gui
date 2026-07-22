import ClickableText from "@renderer/components/ui/clickable-text";
import { Popover, PopoverContent, PopoverTrigger } from "@renderer/components/ui/popover";
import { Appear } from "@renderer/components/ui/routes/animated-content";
import { Spinner } from "@renderer/components/ui/spinner";
import { useIPC } from "@renderer/hooks/use-ipc";
import { trpc } from "@renderer/lib/api/trpc-link";
import { Bell, DotIcon } from "lucide-react";
import prettyBytes from "pretty-bytes";
import { useMemo } from "react";
import { toast } from "sonner";
import { useApp } from "./app-context";
import { useNotifications } from "./notifications-context";
import { useYtdl } from "./ytdl-context";

export default function StatusBar() {
	const { settings } = useApp();
	const { status } = useYtdl();
	const { notifications, unreadCount, isOpen, setOpen, clear } = useNotifications();
	const [data, { refetch }] = trpc.ytdl.stats.useSuspenseQuery();
	const { mutateAsync: quitAndUpdate } = trpc.internals.quitAndInstallUpdate.useMutation({
		onError(error, variables, context) {
			toast.error(error.message);
		},
	});
	const diskUsage = useMemo(() => data && prettyBytes(data.completedUsage), [data]);
	const [updateAvailable, setUpdateAvailable] = useIPC("update-available");
	const [updateChecking, setUpdateChecking] = useIPC("update-checking");
	const [updateProgress, setUpdateProgress] = useIPC("update-download-progress");
	const [updateDone, setUpdateDone] = useIPC("update-download-done");
	const latestYtdlpStatus = useMemo(() => status[0], [status]);
	const noStateOrInvalidStatus = useMemo(
		() => !latestYtdlpStatus || ["done", "deleted"].includes(latestYtdlpStatus.state),
		[latestYtdlpStatus],
	);
	trpc.ytdl.status.useSubscription(undefined, {
		onData(data) {
			if (data.state === "done") refetch();
		},
	});
	return (
		<div className='grid items-center grid-cols-[140px_200px_1fr] h-10 shrink-0 overflow-hidden border-t border-t-border flex-auto text-xs text-muted-foreground pl-4 select-none'>
			<Appear className='flex gap-1 items-center'>
				<span>Disk usage:</span>
				<span>{diskUsage}</span>
			</Appear>
			<div className='flex items-center gap-2'>
				<Appear className='flex gap-1 items-center'>
					<span>Total:</span>
					<span>{data.overallCount}</span>
				</Appear>
				<DotIcon className='size-2 text-muted-foreground' />
				<Appear className='flex gap-1 items-center'>
					<span>Videos:</span>
					<span>{data.count.video}</span>
				</Appear>
				<DotIcon className='size-2 text-muted-foreground' />
				<Appear className='flex gap-1 items-center'>
					<span>Audios:</span>
					<span>{data.count.audio}</span>
				</Appear>
			</div>
			<div className='flex items-center gap-4 justify-end'>
				{updateChecking && !updateAvailable ? (
					<>
						<Appear className='animate-pulse'>Checking for updates...</Appear>
						<div className='h-10 w-px bg-muted'></div>
					</>
				) : updateDone ? (
					<>
						<ClickableText onClick={() => quitAndUpdate()}>
							Click to restart to install update.
						</ClickableText>
						<div className='h-10 w-px bg-muted'></div>
					</>
				) : updateProgress && !updateDone ? (
					<>
						<Appear>
							Downloading... {(updateProgress.percent as number).toFixed(2).padStart(6, " ")}%
						</Appear>
						<div className='h-10 w-px bg-muted'></div>
					</>
				) : updateAvailable && !updateDone ? (
					<>
						<Appear>Update Available, preparing...</Appear>
						<div className='h-10 w-px bg-muted'></div>
					</>
				) : null}
				{noStateOrInvalidStatus ? (
					<Appear>YTDLP: {settings.ytdlp?.version ?? "..."}</Appear>
				) : latestYtdlpStatus.state === "progressing" ? (
					<Appear className='flex items-center gap-1'>
						<span>YTDLP:</span>
						<Spinner size={"xs"} />
					</Appear>
				) : (
					<Appear>YTDLP: {latestYtdlpStatus.state}</Appear>
				)}
				<Popover
					open={isOpen}
					onOpenChange={(open) => {
						setOpen(open);
					}}>
					<PopoverTrigger asChild>
						<button
							type='button'
							className='relative h-10 w-12 shrink-0 border-l border-l-border/60 bg-background hover:bg-muted/50 cursor-pointer transition-colors'
							aria-label='Open notification center'>
							<Bell className='mx-auto size-4 text-foreground/90' />
							{unreadCount > 0 && (
								<span className='absolute top-1 right-1 min-w-4 h-4 rounded-full bg-destructive px-1 text-[10px] leading-4 text-destructive-foreground text-center font-medium'>
									{unreadCount > 99 ? "99+" : unreadCount}
								</span>
							)}
						</button>
					</PopoverTrigger>
					<PopoverContent
						side='top'
						sideOffset={0}
						alignOffset={-16}
						align='end'
						className='w-96 p-0 mb-0 rounded-b-none rounded-tr-none data-[state=open]:slide-in-from-bottom-2 data-[state=closed]:slide-out-to-bottom-2 data-[state=open]:zoom-in-100 data-[state=closed]:zoom-out-100'>
						<div className='flex items-center justify-between border-b border-border px-3 py-2'>
							<span className='text-sm font-medium text-foreground'>Notifications</span>
							<ClickableText onClick={clear}>Clear</ClickableText>
						</div>
						<div className='max-h-72 overflow-y-auto'>
							{notifications.length ? (
								notifications.map((notification) => (
									<div
										key={notification.id}
										className='border-b border-border/60 px-3 py-2 last:border-b-0'>
										<div className='text-sm text-foreground'>{notification.message}</div>
										{notification.description && (
											<div className='text-xs text-muted-foreground break-all'>
												{notification.description}
											</div>
										)}
										<div className='text-[10px] text-muted-foreground mt-1'>
											{new Date(notification.createdAt).toLocaleTimeString()}
										</div>
									</div>
								))
							) : (
								<div className='px-3 py-4 text-xs text-muted-foreground'>
									No notifications in this session.
								</div>
							)}
						</div>
					</PopoverContent>
				</Popover>
			</div>
		</div>
	);
}
