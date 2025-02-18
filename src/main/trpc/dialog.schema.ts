import { z } from "zod";

export const dialogSchema = z.object({
  closeable: z.boolean().default(true),
  closeWithParent: z.boolean().default(false),
  title: z.string().nullish()
}).partial().default({});
export type DialogSchema = z.infer<typeof dialogSchema>
