const AsyncFunction = (async () => {}).constructor;

export function isAsyncFunction(asyncFn: typeof AsyncFunction) {
	return asyncFn instanceof AsyncFunction === true;
}
