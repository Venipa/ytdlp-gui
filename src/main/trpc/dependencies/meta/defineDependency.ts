export type SupportedPlatform = "win32" | "linux" | "darwin" | "all";
export type SupportedArch = "x64" | "arm64" | "all";
export type DependencySourceKind = "archive" | "binary";

export interface DependencySource {
	readonly id: string;
	readonly url: string;
	readonly kind: DependencySourceKind;
	readonly archiveType?: "zip" | "tar";
	readonly exports?: readonly string[];
}

export interface DependencyTarget {
	readonly platform: SupportedPlatform;
	readonly arch: SupportedArch;
	readonly sources: readonly DependencySource[];
}

export interface DependencyRelease {
	readonly version: string;
	readonly targets: readonly DependencyTarget[];
}

export interface DependencyDefinition<TDependencyKey extends string = string> {
	readonly key: TDependencyKey;
	readonly name: string;
	readonly description: string;
	readonly releases: readonly DependencyRelease[];
}

export function defineDependency<TDependencyKey extends string>(definition: DependencyDefinition<TDependencyKey>): DependencyDefinition<TDependencyKey> {
	return definition;
}
