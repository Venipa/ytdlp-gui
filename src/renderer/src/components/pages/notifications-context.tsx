import { RouterOutput } from "@main/api";
import { trpc } from "@renderer/lib/api/trpc-link";
import { logger } from "@shared/logger";
import { Context, Provider, createContext, useContext, useMemo, useState } from "react";
import { toast } from "sonner";

type SessionNotification = RouterOutput["notifications"]["history"][number];

type NotificationCenterContext = {
	notifications: SessionNotification[];
	unreadCount: number;
	isOpen: boolean;
	setOpen: (open: boolean) => void;
	clear: () => void;
};

type NotificationCenterContextType = Context<NotificationCenterContext>;

const notificationContext: NotificationCenterContextType = createContext({} as NotificationCenterContext);

const useNotifications = (): NotificationCenterContext => useContext(notificationContext);

const notifyRenderer = (notification: SessionNotification): void => {
	if (notification.type === "default") {
		toast(notification.message, { description: notification.description ?? undefined });
		return;
	}
	toast[notification.type](notification.message, {
		description: notification.description ?? undefined,
	});
};

const NotificationCenterProvider: Provider<NotificationCenterContext> = (({ value, ...props }) => {
	const [history] = trpc.notifications.history.useSuspenseQuery();
	const [notifications, setNotifications] = useState<SessionNotification[]>(history);
	const [unreadCount, setUnreadCount] = useState<number>(0);
	const [isOpen, setIsOpen] = useState<boolean>(false);

	const setOpen = useMemo(
		() => (open: boolean) => {
			setIsOpen(open);
			if (open) {
				setUnreadCount(0);
			}
		},
		[],
	);

	trpc.notifications.stream.useSubscription(undefined, {
		onData(notification) {
			setNotifications((current) => [notification, ...current]);
			setUnreadCount((current) => (isOpen ? 0 : current + 1));
			notifyRenderer(notification);
			if (notification.type === "error") {
				logger.error("Notification error emitted", {
					message: notification.message,
					description: notification.description,
				});
			}
		},
	});

	const clear = useMemo(
		() => () => {
			setNotifications([]);
			setUnreadCount(0);
		},
		[],
	);
	return (
		<notificationContext.Provider
			value={{
				...value,
				notifications,
				unreadCount,
				isOpen,
				setOpen,
				clear,
			}}
			{...props}
		/>
	);
}) as Provider<NotificationCenterContext>;

export { NotificationCenterProvider, useNotifications };
