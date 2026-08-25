import { appStore } from "@main/stores/app/app.store";

export type ConcurrencyMode = "global" | "domain";

export interface ConcurrencyPolicy {
	mode: ConcurrencyMode;
	limit: number;
}

function normalizeLimit(value: number): number {
	if (!Number.isFinite(value) || value < 1) return 1;
	return Math.floor(value);
}

export function getConcurrencyPolicy(): ConcurrencyPolicy {
	const features = appStore.store.features;
	const mode = features.concurrencyMode === "domain" ? "domain" : "global";
	const configuredLimit = mode === "domain" ? features.concurrentDownloadsPerDomain : features.concurrentDownloads;
	return {
		mode,
		limit: normalizeLimit(configuredLimit),
	};
}

export function getConcurrencyDomain(url: string): string {
	try {
		const hostname = new URL(url).hostname.toLowerCase().replace(/^www\./, "");
		const labels = hostname.split(".").filter(Boolean);
		return labels.length > 2 ? labels.slice(-2).join(".") : hostname;
	} catch {
		return url.toLowerCase();
	}
}
