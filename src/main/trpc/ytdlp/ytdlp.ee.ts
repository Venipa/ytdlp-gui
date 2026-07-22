import EventEmitter from "events";
import { notificationCenter } from "../notifications/notification-center";
import { NotificationType } from "../notifications/notifications.types";
export const ytdlpEvents = new EventEmitter();

export const pushToastToClient = (message: string, type: NotificationType = "default", description?: string) => {
	return notificationCenter.push({ message, type, description });
};
ytdlpEvents.setMaxListeners(10000);
export {};
