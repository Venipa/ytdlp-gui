/**
 * Array helpers are defined non-enumerable on purpose. Plain assignment to a
 * built-in prototype creates an enumerable property, so `for...in` walks pick
 * it up as if it were data. electron-conf deep clones store values with
 * `for...in`, which copied these functions onto every stored array as own
 * properties and made any reply containing one impossible to send over IPC
 * ("Failed to serialize arguments").
 */
const arrayExtensions = {
	clamp(this: unknown[], min: number, max: number) {
		return Math.max(min, Math.min(this.length, max));
	},
	clampMax(this: unknown[], max: number) {
		return Math.min(this.length, max);
	},
	clampMin(this: unknown[], min: number) {
		return Math.max(min, this.length);
	},
	insert(this: unknown[], index: number, item: unknown) {
		return [...this.slice(0, index), item, ...this.slice(index)];
	},
};

for (const [name, value] of Object.entries(arrayExtensions)) {
	Object.defineProperty(Array.prototype, name, {
		value,
		enumerable: false,
		writable: true,
		configurable: true,
	});
}

export {};
