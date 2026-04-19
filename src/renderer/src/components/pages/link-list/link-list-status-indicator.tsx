import { ProgressCircle } from "@renderer/components/ui/progress-circle";
import { Spinner } from "@renderer/components/ui/spinner";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { LucideArrowDownToDot, LucideCheck, LucideListPlus, LucideSquare, LucideX } from "lucide-react";
import { YTDLDownloadStatus } from "ytdlp-gui/types";

interface LinkListStatusIndicatorProps {
	cancelled: boolean;
	completed: boolean;
	downloading: boolean;
	error: unknown;
	queued: boolean;
	status?: YTDLDownloadStatus;
}

export default function LinkListStatusIndicator({ cancelled, completed, downloading, error, queued, status }: LinkListStatusIndicatorProps): JSX.Element {
	if (error) {
		return (
			<QTooltip side='right' content={"An error occurred while downloading."}>
				<div className='size-5 p-1 flex flex-col items-center justify-center border-2 border-destructive/40 bg-destructive rounded-full'>
					<LucideX className='stroke-[4px] stroke-destructive-foreground' />
				</div>
			</QTooltip>
		);
	}

	if (cancelled) {
		return (
			<div className='size-5 p-1 flex flex-col items-center justify-center border-2 border-muted rounded-full'>
				<LucideSquare className='stroke-none fill-current' />
			</div>
		);
	}

	if (completed) {
		return (
			<div className='size-5 p-1 flex flex-col items-center justify-center bg-green-500 text-white rounded-full'>
				<LucideCheck className='stroke-[4px]' />
			</div>
		);
	}

	if (queued) {
		return (
			<QTooltip className='cursor-default' content={"Queued"} side='right'>
				<div className='flex flex-col items-center justify-center size-10 relative'>
					<LucideListPlus className='size-5 text-secondary-foreground' />
				</div>
			</QTooltip>
		);
	}

	if (downloading && status) {
		return (
			<QTooltip className='cursor-default' content={"Download Progress"} side='right'>
				<div className='flex flex-col items-center justify-center size-10 relative'>
					<ProgressCircle
						min={0}
						max={100}
						value={status.percent ?? 0}
						className='h-6'
						gaugePrimaryColor='rgb(225 225 225)'
						gaugeSecondaryColor='rgba(120, 120, 120, 0.1)'
						showValue={false}
					/>
					<LucideArrowDownToDot className='absolute size-3.5 text-secondary-foreground animate-pulse' />
				</div>
			</QTooltip>
		);
	}

	return <Spinner />;
}
