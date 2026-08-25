import type { AppRouter } from "@main/api";
import { isProduction } from "@shared/config";
import { createTRPCReact } from "@trpc/react-query";
import { ipcLink } from "electron-trpc/renderer";
import superjson from "superjson";

export const trpc = createTRPCReact<AppRouter>();
export const client = trpc.createClient({
	transformer: superjson,
	links: [ipcLink()],
});
if (!isProduction) (window as any).trpc = client;
