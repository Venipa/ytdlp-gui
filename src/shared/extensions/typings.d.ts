declare module "zod" {
	interface ZodType<T = any> {
		fallback<U = T>(fallback: U): this;
	}
}

declare global {
	interface String {
		/**
		 * replaces `:<key>` with `args[key]`
		 */
		interpolate<T extends Record<string, unknown>>(args: T): `${string}${keyof T extends never ? "" : `:${keyof T extends string ? string & keyof T : never}`}`;
	}
}

export {};
