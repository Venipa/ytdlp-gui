import { ZodObject, ZodType } from "zod";
function fallback<T extends ZodType>(this: T, fallback: any) {
	return this.nullish().transform((v) => v ?? fallback);
}
// function deepFallback<T extends ZodType<T>>(this: T, fallback: any): T {
// 		if (fallback && typeof fallback === "object") {
// 			for (const [key, value] of Object.entries(fallback)) {
// 				if (key in this) {
// 					if (typeof fallback === "object" && !Array.isArray(value)) {
// 						this[key] = (this[key] as ZodType<T>).fallback(value).deepFallback(value);
// 					} else {
// 						this[key] = (this[key] as ZodType<T>).fallback(value);
// 					}
// 				}
// 			}
// 		}
// 		return this;
// }
ZodObject.prototype.fallback = ZodType.prototype.fallback = fallback;
// ZodObject.prototype.deepFallback = ZodType.prototype.deepFallback = deepFallback;

export {};
