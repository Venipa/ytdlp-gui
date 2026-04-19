import { trpc } from "@renderer/lib/api/trpc-link";
import { PropsWithChildren, createContext, useCallback, useContext, useMemo, useState } from "react";
import { YTDLItem } from "ytdlp-gui/types";

type LinkListItemId = YTDLItem["id"];

interface LinkListActionsContextValue {
	deletingId: LinkListItemId | null;
	cancelById: (id: LinkListItemId) => Promise<unknown>;
	deleteById: (id: LinkListItemId, deleteFile: boolean) => Promise<unknown>;
	openParentPath: (path: string) => Promise<unknown>;
	retryById: (id: LinkListItemId) => Promise<unknown>;
}

const LinkListActionsContext = createContext<LinkListActionsContextValue | null>(null);

function isNotFoundError(error: unknown): boolean {
	if (!error || typeof error !== "object") return false;

	const maybeError = error as {
		data?: { code?: string };
		message?: string;
		shape?: { message?: string };
	};

	const code = maybeError.data?.code;
	if (code === "NOT_FOUND") return true;

	const message = `${maybeError.message ?? ""} ${maybeError.shape?.message ?? ""}`.toLowerCase();
	return message.includes("not found");
}

export default function LinkListProvider({ children }: PropsWithChildren): JSX.Element {
	const [deletingId, setDeletingId] = useState<LinkListItemId | null>(null);
	const utils = trpc.useUtils();
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();
	const { mutateAsync: retryFromId } = trpc.ytdl.retry.useMutation();
	const { mutateAsync: cancelFromId } = trpc.ytdl.cancel.useMutation();
	const { mutateAsync: deleteFromId } = trpc.ytdl.delete.useMutation();

	const openParentPath = useCallback(
		(path: string) => {
			return openPath({ path, openParent: true });
		},
		[openPath],
	);

	const retryById = useCallback(
		(id: LinkListItemId) => {
			return retryFromId(id);
		},
		[retryFromId],
	);

	const cancelById = useCallback(
		(id: LinkListItemId) => {
			return cancelFromId(id);
		},
		[cancelFromId],
	);

	const deleteById = useCallback(
		async (id: LinkListItemId, deleteFile: boolean) => {
			setDeletingId(id);
			try {
				return await deleteFromId({ id, deleteFile });
			} catch (error: unknown) {
				if (isNotFoundError(error)) {
					utils.ytdl.list.setData(undefined, (currentItems) => {
						if (!currentItems?.length) return currentItems ?? [];
						return currentItems.filter((item) => item.id !== id);
					});
					return;
				}
				throw error;
			} finally {
				setDeletingId((previousId) => (previousId === id ? null : previousId));
			}
		},
		[deleteFromId, utils.ytdl.list],
	);

	const value = useMemo<LinkListActionsContextValue>(
		() => ({
			deletingId,
			cancelById,
			deleteById,
			openParentPath,
			retryById,
		}),
		[cancelById, deleteById, deletingId, openParentPath, retryById],
	);

	return <LinkListActionsContext.Provider value={value}>{children}</LinkListActionsContext.Provider>;
}

export function useLinkListActions(): LinkListActionsContextValue {
	const context = useContext(LinkListActionsContext);
	if (!context) {
		throw new Error("useLinkListActions must be used within LinkListProvider");
	}
	return context;
}
