import { z } from "zod";

export const NotificationTypeSchema = z.enum(["default", "info", "success", "warning", "error"]);

export const NotificationItemSchema = z.object({
	id: z.string(),
	message: z.string(),
	description: z.string().nullable().optional(),
	type: NotificationTypeSchema.default("default"),
	createdAt: z.string(),
});

export const PushNotificationInputSchema = z.object({
	message: z.string(),
	description: z.string().nullable().optional(),
	type: NotificationTypeSchema.default("default"),
});

export type NotificationType = z.infer<typeof NotificationTypeSchema>;
export type NotificationItem = z.infer<typeof NotificationItemSchema>;
export type PushNotificationInput = z.infer<typeof PushNotificationInputSchema>;
