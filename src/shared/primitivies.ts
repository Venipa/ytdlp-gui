Array.prototype.clamp = function (min: number, max: number) {
	return Math.max(min, Math.min(this.length, max));
};
Array.prototype.clampMax = function (max: number) {
	return Math.min(this.length, max);
};
Array.prototype.clampMin = function (min: number) {
	return Math.max(min, this.length);
};
Array.prototype.insert = function (index: number, item: any) {
	return [...this.slice(0, index), item, ...this.slice(index)];
};

export {};
