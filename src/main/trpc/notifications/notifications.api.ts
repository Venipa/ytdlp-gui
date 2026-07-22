import { observable } from "@trpc/server/observable";
import { publicProcedure, router } from "../core/trpc";
import { notificationCenter } from "./notification-center";
import { NotificationItem } from "./notifications.types";

export const notificationsRouter = router({
	history: publicProcedure.query(() => {
		return notificationCenter.list();
	}),
	stream: publicProcedure.subscription(() => {
		return observable<NotificationItem>((emit) => {
			const unsubscribe = notificationCenter.onNotification((item) => {
				emit.next(item);
			});

			return () => {
				unsubscribe();
			};
		});
	}),
});
