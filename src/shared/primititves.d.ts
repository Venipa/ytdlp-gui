declare global {
	interface Array {
		clamp(min: number, max: number): number;
		clampMax(max: number): number;
		clampMin(min: number): number;
		insert(index: number, item: any): any[];
	}
}
export {};
