import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import { Button } from "@renderer/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@renderer/components/ui/popover";
import { LucideFileX } from "lucide-react";
import { useState } from "react";

interface LinkListDeletePopoverProps {
	loading: boolean;
	onDeleteEntry: () => void;
	onDeleteEntryAndFile: () => void;
}

export default function LinkListDeletePopover({ loading, onDeleteEntry, onDeleteEntryAndFile }: LinkListDeletePopoverProps): JSX.Element {
	const [open, setOpen] = useState<boolean>(false);

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<ButtonLoading
					variant={"ghost"}
					size={"sm"}
					className='px-2 text-red-500 hover:text-red-400 opacity-0 group-hover/item:opacity-100'
					onClick={(event) => {
						event.stopPropagation();
					}}
					loading={loading}
					fixWidth>
					<LucideFileX className='stroke-current' />
				</ButtonLoading>
			</PopoverTrigger>
			<PopoverContent className='w-90 p-3' align='end'>
				<div className='flex flex-col gap-3'>
					<div className='text-sm font-medium'>Delete downloaded file too?</div>
					<div className='text-xs text-muted-foreground'>
						Choose whether to also remove the file from disk. If you skip this, only the download entry is removed from the list.
					</div>
					<div className='flex items-center justify-end gap-2'>
						<Button
							variant='outline'
							size='sm'
							onClick={(event) => {
								event.preventDefault();
								event.stopPropagation();
								setOpen(false);
							}}>
							Cancel
						</Button>
						<Button
							variant='outline'
							size='sm'
							onClick={(event) => {
								event.preventDefault();
								event.stopPropagation();
								onDeleteEntry();
								setOpen(false);
							}}>
							Keep File
						</Button>
						<Button
							size='sm'
							onClick={(event) => {
								event.preventDefault();
								event.stopPropagation();
								onDeleteEntryAndFile();
								setOpen(false);
							}}>
							Delete File & Entry
						</Button>
					</div>
				</div>
			</PopoverContent>
		</Popover>
	);
}
