const DEBUG_FLAGS = process.env.DEBUG?.split(",").map((flag) => flag.trim()) ?? [];

export const checkDebugFlag = (flag: string) => DEBUG_FLAGS.includes(flag);
