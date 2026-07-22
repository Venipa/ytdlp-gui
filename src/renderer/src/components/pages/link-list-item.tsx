import { useLinkListActions } from "@renderer/components/pages/link-list/link-list-context";
import LinkListRow from "@renderer/components/pages/link-list/link-list-row";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuTrigger,
} from "@renderer/components/ui/context-menu";
import { toast } from "sonner";

type LinkListItemProps = React.ComponentProps<typeof LinkListRow>;

const copyToClipboard = (value: string, label: string): void => {
	navigator.clipboard
		.writeText(value)
		.then(() => toast(`Copied ${label}`))
		.catch(() => toast.error(`Failed to copy ${label}`));
};

export function LinkListItem(props: LinkListItemProps): JSX.Element {
	const item = props.row.original;
	const { id, url, title, state, filepath } = item;
	const { cancelById, deleteById, openParentPath, retryById } = useLinkListActions();

	const canRetry = state === "error" || state === "cancelled";
	const canCancel = state === "downloading" || state === "fetching_meta";
	const canOpenFolder = state === "completed" && Boolean(filepath);

	return (
		<ContextMenu>
			<ContextMenuTrigger asChild>
				<div>
					<LinkListRow {...props} />
				</div>
			</ContextMenuTrigger>
			<ContextMenuContent className='w-56'>
				<ContextMenuItem onSelect={() => copyToClipboard(String(title ?? ""), "title")}>
					Copy title
				</ContextMenuItem>
				<ContextMenuItem onSelect={() => copyToClipboard(String(url ?? ""), "url")}>Copy URL</ContextMenuItem>
				<ContextMenuSeparator />
				<ContextMenuItem disabled={!canRetry} onSelect={() => canRetry && void retryById(id)}>
					Retry
				</ContextMenuItem>
				<ContextMenuItem disabled={!canCancel} onSelect={() => canCancel && void cancelById(id)}>
					Cancel
				</ContextMenuItem>
				<ContextMenuItem
					disabled={!canOpenFolder}
					onSelect={() => canOpenFolder && filepath && void openParentPath(filepath)}>
					Open Folder
				</ContextMenuItem>
				<ContextMenuSeparator />
				<ContextMenuItem onSelect={() => void deleteById(id, false)}>Delete Entry</ContextMenuItem>
				<ContextMenuItem disabled={!canOpenFolder} onSelect={() => canOpenFolder && void deleteById(id, true)}>
					Delete Entry + File
				</ContextMenuItem>
			</ContextMenuContent>
		</ContextMenu>
	);
}

export default LinkListItem;
