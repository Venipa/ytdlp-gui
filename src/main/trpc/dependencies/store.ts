import { createEncryptedStore } from "@shared/electron/store/createYmlStore";
import { dependencyDefinitions } from "./meta";

export interface DependencyInstallState {
	readonly path: string;
	readonly version: string;
	readonly files: readonly string[];
	readonly usedSpaceBytes: number;
	readonly updatedAt: string;
}

export type DependencyStoreData = Partial<Record<keyof typeof dependencyDefinitions, DependencyInstallState>>;
export const dependencyStore = createEncryptedStore<DependencyStoreData>("dependencies", {
	defaults: {},
});
