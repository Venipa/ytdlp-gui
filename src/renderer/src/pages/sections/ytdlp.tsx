import AddLink from "@renderer/components/pages/add-link";
import LinkList from "@renderer/components/pages/link-list";
import PlainLayout from "@renderer/components/pages/plain-layout";
import { LucideHardDriveDownload } from "lucide-react";

export const meta = {
	title: "Downloads",
	icon: LucideHardDriveDownload,
	index: 0,
	show: true,
	customLayout: PlainLayout,
};
const Icon = meta.icon;
export default function YTDLPTab() {
	return (
		<div className='flex flex-col gap-6 p-0 h-full overflow-hidden'>
			<div className='flex flex-col gap-y-4 pt-10 h-full'>
				<div className='flex items-start justify-between px-4'>
					<div className='flex items-center gap-2'>
						<Icon className='size-5' />
						<h1 className='text-lg font-semibold'>Downloads</h1>
					</div>
				</div>
				<div className='flex flex-col flex-auto min-h-0'>
					<AddLink />
					<LinkList className='grow' />
				</div>
			</div>
		</div>
	);
}
