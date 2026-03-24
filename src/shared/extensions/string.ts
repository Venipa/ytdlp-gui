String.prototype.interpolate = function <T extends Record<string, unknown>>(
	args: T,
): `${string}${keyof T extends never ? "" : `:${keyof T extends string ? string & keyof T : never}`}` {
	const str = String(this);
	return str.replace(/:([a-zA-Z0-9_]+)/g, (match, key) => {
		return key in args ? String(args[key]) : match;
	}) as `${string}${keyof T extends never ? "" : `:${keyof T extends string ? string & keyof T : never}`}`;
};
export {};
