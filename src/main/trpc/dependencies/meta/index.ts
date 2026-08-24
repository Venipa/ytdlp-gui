import { ffmpegDependency } from "./ffmpeg";
import { ytdlpDependency } from "./ytdlp";

export const dependencyDefinitions = {
	ffmpeg: ffmpegDependency,
	ytdlp: ytdlpDependency,
} as const;

export type DependencyKey = keyof typeof dependencyDefinitions;
export const dependencyKeys = Object.keys(dependencyDefinitions) as [DependencyKey, ...DependencyKey[]];
