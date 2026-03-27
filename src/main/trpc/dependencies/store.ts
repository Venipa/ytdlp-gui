import { createYmlStore } from "@shared/electron/store/createYmlStore";
import z from "zod";
import { type DependencyKey, dependencyKeys } from "./meta";

export interface DependencyInstallState {
	readonly path: string;
	readonly version: string;
	readonly files: readonly string[];
	readonly usedSpaceBytes: number;
	readonly updatedAt: string;
}

export const dependencyStoreSchema = z
	.record(
		z.enum(dependencyKeys as [DependencyKey, ...DependencyKey[]]),
		z
			.object({
				path: z.string(),
				version: z.string(),
				files: z.array(z.string()).fallback([]),
				usedSpaceBytes: z.number().fallback(0),
				updatedAt: z.string(),
			})
			.fallback(null),
	)
	.fallback({});

export type DependencyStoreData = Partial<Record<DependencyKey, DependencyInstallState>>;
export const dependencyStore = createYmlStore<DependencyStoreData>("dependencies", {
	defaults: dependencyStoreSchema.parse({}),
});
