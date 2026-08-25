import type { RouterOutput } from "@main/api";
import { Badge } from "@renderer/components/ui/badge";
import { Button } from "@renderer/components/ui/button";
import { Checkbox } from "@renderer/components/ui/checkbox";
import { Label } from "@renderer/components/ui/label";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetFooter,
	SheetHeader,
	SheetTitle,
} from "@renderer/components/ui/sheet";
import { Spinner } from "@renderer/components/ui/spinner";
import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import { Switch } from "@renderer/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@renderer/components/ui/table";
import { Textarea } from "@renderer/components/ui/textarea";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { trpc } from "@renderer/lib/api/trpc-link";
import { cn } from "@renderer/lib/ui/utils";
import { throwErrorToast } from "@shared/trpc/error";
import {
	type RowSelectionState,
	type SortingState,
	createColumnHelper,
	flexRender,
	getCoreRowModel,
	getSortedRowModel,
	useReactTable,
} from "@tanstack/react-table";
import { formatDistanceToNow } from "date-fns";
import { LucideClipboardCopy, LucideCookie, LucideEdit2, LucideImport, LucidePlus } from "lucide-react";
import { useId, useState } from "react";
import { toast } from "sonner";

export const meta = {
	title: "Cookies",
	icon: LucideCookie,
	index: 5,
	show: true,
	customLayout: true,
};

type CookieFileMeta = RouterOutput["cookies"]["list"]["files"][number];
type CookieFileDetail = RouterOutput["cookies"]["get"];

/**
 * Devtools console snippet that downloads the current site's cookies as a Netscape file.
 * Only reads cookies exposed to JavaScript, httpOnly cookies stay invisible to the page.
 */
const COOKIE_EXPORT_SNIPPET = `(async () => {
  const host = location.hostname;
  const fallbackExpiry = Math.floor(Date.now() / 1000) + 60 * 60 * 24 * 30;
  const cookies = window.cookieStore
    ? await cookieStore.getAll()
    : document.cookie.split("; ").filter(Boolean).map((pair) => {
        const split = pair.indexOf("=");
        return { name: pair.slice(0, split), value: pair.slice(split + 1) };
      });
  const lines = ["# Netscape HTTP Cookie File"];
  for (const cookie of cookies) {
    const domain = cookie.domain || "." + host;
    const secure = (cookie.secure ?? location.protocol === "https:") ? "TRUE" : "FALSE";
    const expires = cookie.expires ? Math.floor(cookie.expires / 1000) : fallbackExpiry;
    lines.push([domain, domain.startsWith(".") ? "TRUE" : "FALSE", cookie.path || "/", secure, expires, cookie.name, cookie.value].join("\\t"));
  }
  const url = URL.createObjectURL(new Blob([lines.join("\\n") + "\\n"], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = "cookies-" + host + ".txt";
  link.click();
  URL.revokeObjectURL(url);
  console.log("exported " + (lines.length - 1) + " cookies for " + host);
})();`;

const columnHelper = createColumnHelper<CookieFileMeta>();
const Icon = meta.icon;

type CookieFileEditorProps = {
	file: CookieFileDetail;
	onClose: () => void;
};

function CookieFileEditor({ file, onClose }: CookieFileEditorProps) {
	const contentId = useId();
	const [content, setContent] = useState(() => file.content);
	const utils = trpc.useUtils();
	const updateFile = trpc.cookies.update.useMutation({
		onError: throwErrorToast,
		onSuccess: async () => {
			await utils.cookies.list.invalidate();
			await utils.cookies.get.invalidate({ id: file.id });
			toast.success("Cookie file saved");
			onClose();
		},
	});
	const deleteFile = trpc.cookies.delete.useMutation({
		onError: throwErrorToast,
		onSuccess: async () => {
			await utils.cookies.list.invalidate();
			toast.success("Cookie file removed");
			onClose();
		},
	});
	const isBusy = updateFile.isLoading || deleteFile.isLoading;
	const canSave = content !== file.content;

	return (
		<div className='flex flex-col gap-4 h-full min-h-0'>
			<SheetHeader>
				<SheetTitle>{file.name}</SheetTitle>
				<SheetDescription>
					{file.cookieCount} cookies across {file.domainCount} domains. Netscape format.
				</SheetDescription>
			</SheetHeader>
			<div className='flex flex-col gap-2 min-h-0 flex-auto'>
				<Label htmlFor={contentId}>Cookie file</Label>
				<Textarea
					id={contentId}
					plain
					spellCheck={false}
					value={content}
					onChange={(event) => {
						setContent(event.target.value);
					}}
					className='-mx-6 rounded-none flex w-auto min-h-48 flex-auto resize-none whitespace-pre font-mono text-xs'
				/>
			</div>
			<SheetFooter className='gap-2'>
				<Button
					type='button'
					variant='destructive'
					disabled={isBusy}
					onClick={() => {
						void deleteFile.mutateAsync({ ids: [file.id] });
					}}>
					{deleteFile.isLoading ? <Spinner size='sm' /> : "Delete"}
				</Button>
				<Button type='button' variant='outline' disabled={isBusy} onClick={onClose}>
					Cancel
				</Button>
				<Button
					type='button'
					disabled={isBusy || !canSave}
					onClick={() => {
						void updateFile.mutateAsync({ id: file.id, content });
					}}>
					{updateFile.isLoading ? <Spinner size='sm' /> : "Save"}
				</Button>
			</SheetFooter>
		</div>
	);
}

type CookieFileEditorSheetProps = {
	fileId: string | null;
	onOpenChange: (open: boolean) => void;
};

function CookieFileEditorSheet({ fileId, onOpenChange }: CookieFileEditorSheetProps) {
	const open = fileId !== null;
	const { data, isFetching } = trpc.cookies.get.useQuery({ id: fileId ?? "" }, { enabled: open });

	return (
		<Sheet
			open={open}
			onOpenChange={(nextOpen) => {
				if (!nextOpen) onOpenChange(false);
			}}>
			<SheetContent className='flex flex-col gap-6 sm:max-w-[85vw]' side='right'>
				{isFetching || !data ? (
					<SuspenseLoader />
				) : (
					<CookieFileEditor key={data.id} file={data} onClose={() => onOpenChange(false)} />
				)}
			</SheetContent>
		</Sheet>
	);
}

export default function CookiesTab() {
	const enabledId = useId();
	const [sorting, setSorting] = useState<SortingState>([{ id: "updatedAt", desc: true }]);
	const [rowSelection, setRowSelection] = useState<RowSelectionState>({});
	const [editingId, setEditingId] = useState<string | null>(null);
	const utils = trpc.useUtils();
	const { data, isFetching } = trpc.cookies.list.useQuery();
	const files = data?.files ?? [];
	const importFiles = trpc.cookies.importFiles.useMutation({
		onError: throwErrorToast,
		onSuccess: async (result) => {
			if (!result.imported.length) return;
			await utils.cookies.list.invalidate();
			toast.success(`Imported ${result.imported.length} cookie file${result.imported.length === 1 ? "" : "s"}`);
		},
	});
	const createFile = trpc.cookies.create.useMutation({
		onError: throwErrorToast,
		onSuccess: async (created) => {
			await utils.cookies.list.invalidate();
			setEditingId(created.id);
		},
	});
	const setEnabled = trpc.cookies.setEnabled.useMutation({
		onError: throwErrorToast,
		onSuccess: async ({ isEnabled }) => {
			await utils.cookies.list.invalidate();
			toast.success(isEnabled ? "Cookie file enabled" : "Cookie file disabled");
		},
	});
	const activateFiles = trpc.cookies.activate.useMutation({
		onError: throwErrorToast,
		onSuccess: async (result) => {
			await utils.cookies.list.invalidate();
			setRowSelection({});
			if (result.cookiesPath) {
				toast.success("Merged cookies file ready for yt-dlp");
				return;
			}
			toast.error("Selected files had no valid Netscape cookies");
		},
	});

	const selectedIds = Object.entries(rowSelection)
		.filter(([, selected]) => selected)
		.map(([id]) => id);
	const selectedCount = selectedIds.length;
	const hasSelection = selectedCount > 0;

	const columns = [
		columnHelper.display({
			id: "selection",
			enableSorting: false,
			header: ({ table }) => {
				const allSelected = table.getIsAllRowsSelected();
				const someSelected = table.getIsSomeRowsSelected();
				return (
					<div className='flex items-center justify-center'>
						<Checkbox
							size='sm'
							variant='ghost'
							checked={allSelected ? true : someSelected ? "indeterminate" : false}
							aria-label='Select all cookie files'
							onCheckedChange={(checked) => {
								table.toggleAllRowsSelected(checked === true);
							}}
						/>
					</div>
				);
			},
			cell: ({ row }) => (
				<div className='flex items-center justify-center'>
					<Checkbox
						size='sm'
						variant='ghost'
						checked={row.getIsSelected()}
						aria-label={`Select ${row.original.name}`}
						onClick={(event) => event.stopPropagation()}
						onCheckedChange={(checked) => {
							row.toggleSelected(checked === true);
						}}
					/>
				</div>
			),
		}),
		columnHelper.accessor("name", {
			header: "Name",
			cell: ({ row }) => (
				<div className='flex items-center gap-2 min-w-0'>
					<span className='truncate font-medium'>{row.original.name}</span>
					{row.original.isActive ? (
						<Badge variant='secondary' size='sm'>
							Active
						</Badge>
					) : null}
				</div>
			),
		}),
		columnHelper.accessor("type", {
			header: "Type",
			cell: ({ getValue }) => <span className='capitalize text-muted-foreground'>{getValue()}</span>,
		}),
		columnHelper.accessor("cookieCount", {
			header: "Cookies",
			cell: ({ getValue }) => <span className='tabular-nums'>{getValue()}</span>,
		}),
		columnHelper.accessor("domainCount", {
			header: "Domains",
			cell: ({ getValue }) => <span className='tabular-nums'>{getValue()}</span>,
		}),
		columnHelper.accessor("domains", {
			header: "Hosts",
			enableSorting: false,
			cell: ({ row }) => {
				const extra = row.original.domainCount - row.original.domains.length;
				const preview = row.original.domains.join(", ");
				return (
					<span className='truncate text-muted-foreground' title={preview}>
						{preview || "-"}
						{extra > 0 ? ` +${extra}` : ""}
					</span>
				);
			},
		}),
		columnHelper.accessor("updatedAt", {
			header: "Updated",
			cell: ({ getValue }) => (
				<span className='text-muted-foreground whitespace-nowrap'>
					{formatDistanceToNow(getValue(), { addSuffix: true })}
				</span>
			),
		}),
		columnHelper.display({
			id: "actions",
			enableSorting: false,
			header: () => <span className='sr-only'>Actions</span>,
			cell: ({ row }) => (
				<div className='flex justify-end'>
					<QTooltip asChild content='Edit cookie file'>
						<Button
							type='button'
							size='icon'
							variant='ghost'
							aria-label={`Edit ${row.original.name}`}
							onClick={(event) => {
								event.stopPropagation();
								setEditingId(row.original.id);
							}}>
							<LucideEdit2 className='size-4' />
						</Button>
					</QTooltip>
				</div>
			),
		}),
	];

	const table = useReactTable({
		data: files,
		columns,
		state: { sorting, rowSelection },
		onSortingChange: setSorting,
		onRowSelectionChange: setRowSelection,
		getRowId: (file) => file.id,
		enableRowSelection: true,
		getCoreRowModel: getCoreRowModel(),
		getSortedRowModel: getSortedRowModel(),
	});

	return (
		<div className='relative flex flex-col gap-4 p-4 pt-10 h-full min-h-0'>
			<div className='flex items-start justify-between gap-4 shrink-0'>
				<div className='flex items-center gap-2'>
					<Icon className='size-5' />
					<div>
						<h1 className='text-lg font-semibold'>Cookies</h1>
						<p className='text-xs text-muted-foreground'>
							Import Netscape cookie files, edit them, then save a merged file for yt-dlp.
						</p>
					</div>
				</div>
				<div className='flex items-center gap-2 shrink-0'>
					<div className='flex items-center gap-2 pr-2'>
						<Switch
							id={enabledId}
							checked={data?.isEnabled ?? false}
							disabled={!data || setEnabled.isLoading}
							onCheckedChange={(isEnabled) => {
								void setEnabled.mutateAsync(isEnabled);
							}}
						/>
						<Label htmlFor={enabledId} className='text-foreground'>
							Enabled
						</Label>
					</div>
					<QTooltip
						asChild
						className='max-w-72'
						content='Copy the browser console script. Paste it into devtools on the site you need cookies for. It downloads a cookies txt you can import here. Chrome asks you to type "allow pasting" first.'>
						<Button
							type='button'
							size='icon'
							variant='outline'
							onClick={() => {
								void navigator.clipboard.writeText(COOKIE_EXPORT_SNIPPET).then(() => {
									toast.success("Copied browser console script", {
										description:
											"Run it in devtools on the target site, then import the downloaded file",
									});
								});
							}}>
							<LucideClipboardCopy />
							<span className='sr-only'>Copy browser console script</span>
						</Button>
					</QTooltip>
					<QTooltip asChild content='Create a cookie file and edit it here'>
						<Button
							type='button'
							size='icon'
							variant='outline'
							disabled={createFile.isLoading}
							onClick={() => {
								void createFile.mutateAsync();
							}}>
							{createFile.isLoading ? <Spinner size='sm' /> : <LucidePlus />}
							<span className='sr-only'>Create cookie file manually</span>
						</Button>
					</QTooltip>
					<Button
						type='button'
						variant='outline'
						disabled={importFiles.isLoading}
						onClick={() => {
							void importFiles.mutateAsync();
						}}>
						{importFiles.isLoading ? <Spinner size='sm' /> : <LucideImport />}
						Import from file
					</Button>
				</div>
			</div>
			<div className='relative flex-auto min-h-0 border border-border rounded-lg overflow-hidden'>
				{isFetching && files.length === 0 ? (
					<SuspenseLoader className='absolute bg-background inset-0 z-10' />
				) : null}
				{files.length === 0 && !isFetching ? (
					<div className='flex flex-col items-center justify-center h-full gap-2 text-sm text-muted-foreground'>
						<LucideCookie className='size-8 opacity-40' />
						<p>No cookie files yet.</p>
					</div>
				) : (
					<Table>
						<TableHeader className='sticky top-0 bg-background z-10'>
							{table.getHeaderGroups().map((headerGroup) => (
								<TableRow key={headerGroup.id}>
									{headerGroup.headers.map((header) => (
										<TableHead
											key={header.id}
											className={cn(header.column.getCanSort() && "cursor-pointer select-none")}
											onClick={header.column.getToggleSortingHandler()}>
											{header.isPlaceholder
												? null
												: flexRender(header.column.columnDef.header, header.getContext())}
										</TableHead>
									))}
								</TableRow>
							))}
						</TableHeader>
						<TableBody>
							{table.getRowModel().rows.map((row) => (
								<TableRow key={row.id} data-state={row.getIsSelected() ? "selected" : undefined}>
									{row.getVisibleCells().map((cell) => (
										<TableCell key={cell.id} className='max-w-48'>
											{flexRender(cell.column.columnDef.cell, cell.getContext())}
										</TableCell>
									))}
								</TableRow>
							))}
						</TableBody>
					</Table>
				)}
			</div>
			<div
				aria-hidden={!hasSelection}
				className={cn(
					"absolute bottom-6 left-1/2 -translate-x-1/2 z-20 transition-all duration-200 ease-out",
					hasSelection ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2 pointer-events-none",
				)}>
				<div className='flex items-center gap-2 rounded-lg border border-border bg-background/95 shadow-lg px-2 py-2 backdrop-blur'>
					<span className='text-xs text-muted-foreground px-2 tabular-nums min-w-24 text-right'>
						{selectedCount} selected
					</span>
					<Button
						type='button'
						size='sm'
						variant='outline'
						disabled={!hasSelection || activateFiles.isLoading}
						onClick={() => {
							setRowSelection({});
						}}>
						Cancel
					</Button>
					<Button
						type='button'
						size='sm'
						disabled={!hasSelection || activateFiles.isLoading}
						onClick={() => {
							void activateFiles.mutateAsync({ ids: selectedIds });
						}}>
						{activateFiles.isLoading ? <Spinner size='sm' /> : "Save"}
					</Button>
				</div>
			</div>
			<CookieFileEditorSheet fileId={editingId} onOpenChange={(open) => setEditingId(open ? editingId : null)} />
		</div>
	);
}
