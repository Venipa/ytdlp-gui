import { publicProcedure, router } from "@main/trpc/trpc";
import { TRPCError } from "@trpc/server";
import { observable } from "@trpc/server/observable";
import z from "zod";
import { DependencyProgressEvent, dependenciesManager } from "./handler";
import { dependencyKeys } from "./meta";

const dependenciesRouter = router({
	list: publicProcedure.query(() => {
		return dependenciesManager.getDefinitions();
	}),
	getDependency: publicProcedure
		.input(
			z.object({
				key: z.enum(dependencyKeys),
			}),
		)
		.query(async ({ input: { key } }) => {
			const definition = dependenciesManager.getDefinitions().find((item) => item.key === key);
			if (!definition) throw new TRPCError({ code: "NOT_FOUND", message: `${key} dependency not found` });
			return definition;
		}),
	resolveDownload: publicProcedure
		.input(
			z.object({
				key: z.enum(dependencyKeys),
				version: z.string().optional(),
			}),
		)
		.query(({ input: { key, version } }) => {
			try {
				return dependenciesManager.resolveRelease(key, version);
			} catch (error: unknown) {
				throw new TRPCError({
					code: "BAD_REQUEST",
					message: error instanceof Error ? error.message : String(error),
				});
			}
		}),
	download: publicProcedure
		.input(
			z.object({
				key: z.enum(dependencyKeys),
				version: z.string().optional(),
			}),
		)
		.mutation(async ({ input: { key, version } }) => {
			try {
				return await dependenciesManager.downloadDependency(key, version);
			} catch (error: unknown) {
				throw new TRPCError({
					code: "BAD_REQUEST",
					message: error instanceof Error ? error.message : String(error),
				});
			}
		}),
	cancelDownload: publicProcedure
		.input(
			z.object({
				key: z.enum(dependencyKeys),
			}),
		)
		.mutation(({ input: { key } }) => {
			const cancelled = dependenciesManager.cancelDownload(key);
			return { key, cancelled };
		}),
	removeDependency: publicProcedure
		.input(
			z.object({
				key: z.enum(dependencyKeys),
			}),
		)
		.mutation(async ({ input: { key } }) => {
			try {
				return await dependenciesManager.removeDependency(key);
			} catch (error: unknown) {
				throw new TRPCError({
					code: "INTERNAL_SERVER_ERROR",
					message: error instanceof Error ? error.message : String(error),
				});
			}
		}),
	downloadState: publicProcedure.query(() => {
		return dependenciesManager.getActiveDownloads();
	}),
	downloadProgress: publicProcedure
		.input(
			z
				.object({
					key: z.enum(dependencyKeys).optional(),
				})
				.optional(),
		)
		.subscription(({ input }) => {
			return observable<DependencyProgressEvent>((emit) => {
				const handler = (payload: DependencyProgressEvent) => {
					if (input?.key && payload.key !== input.key) return;
					emit.next(payload);
				};

				dependenciesManager.getEvents().on("progress", handler);
				return () => {
					dependenciesManager.getEvents().off("progress", handler);
				};
			});
		}),
});

export default dependenciesRouter;
