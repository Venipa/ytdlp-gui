import { randomUUID } from "crypto";
import EventEmitter from "events";
import { NotificationItem, NotificationType, PushNotificationInputSchema } from "./notifications.types";

const MAX_SESSION_NOTIFICATIONS = 200;

class NotificationCenter {
	private readonly eventEmitter: EventEmitter;
	private readonly history: NotificationItem[];

	constructor() {
		this.eventEmitter = new EventEmitter();
		this.history = [];
	}

	public push(input: { message: string; description?: string | null; type?: NotificationType }): NotificationItem {
		const parsedInput = PushNotificationInputSchema.parse(input);
		const item: NotificationItem = {
			id: randomUUID(),
			message: parsedInput.message,
			description: parsedInput.description ?? null,
			type: parsedInput.type ?? "default",
			createdAt: new Date().toISOString(),
		};
		this.history.unshift(item);
		if (this.history.length > MAX_SESSION_NOTIFICATIONS) {
			this.history.length = MAX_SESSION_NOTIFICATIONS;
		}
		this.eventEmitter.emit("notification", item);
		return item;
	}

	public list(): NotificationItem[] {
		return [...this.history];
	}

	public onNotification(listener: (item: NotificationItem) => void): () => void {
		this.eventEmitter.on("notification", listener);
		return () => this.eventEmitter.off("notification", listener);
	}
}

export const notificationCenter = new NotificationCenter();
