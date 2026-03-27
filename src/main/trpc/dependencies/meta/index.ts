import { ffmpegDependency } from "./ffmpeg";

export const dependencyDefinitions = {
	ffmpeg: ffmpegDependency,
} as const;

export type DependencyKey = keyof typeof dependencyDefinitions;
export const dependencyKeys = Object.keys(dependencyDefinitions) as [DependencyKey, ...DependencyKey[]];
