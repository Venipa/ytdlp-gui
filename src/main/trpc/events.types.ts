import { z } from "zod"

export const EventNameSchema = z.enum(["message", "log"])
export type EventNames = z.infer<typeof EventNameSchema>
