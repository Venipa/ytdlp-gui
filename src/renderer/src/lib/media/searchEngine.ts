import { logger } from "@shared/logger";

type MediaType = "video" | "audio";
type StatusType = "success" | "error" | "active";

export interface SearchQueryFilters {
	type?: MediaType;
	size?: {
		op: "<" | "<=" | ">" | ">=" | "=";
		value: number;
		unit: "b" | "kb" | "mb" | "gb";
	};
	status?: StatusType;
	text?: string;
}

/** Helper to parse a size string like "100mb" to {value: 100, unit: "mb"} */
function parseSize(sizeStr: string): { value: number; unit: "b" | "kb" | "mb" | "gb" } | null {
	const match = sizeStr.match(/^(\d+(?:\.\d+)?)(b|kb|mb|gb)$/i);
	if (!match) return null;
	return {
		value: parseFloat(match[1]),
		unit: match[2].toLowerCase() as "b" | "kb" | "mb" | "gb",
	};
}
export type SearchItem<T> = T & { type: MediaType; filesize: number; status: StatusType; [k: string]: any };

/**
 * Parses the search string into filters and free text.
 * Supports:
 *   type:video|audio|auto
 *   size:<100mb, <=10gb, etc
 *   status:success|error|active
 * Free text (not matched to a filter) appears in .text
 */
export class SearchEngine {
	parse(query: string): SearchQueryFilters {
		const result: SearchQueryFilters = {};
		let text = query.trim();

		// Patterns for our supported filters
		// type:<value>
		const typeMatch = text.match(/(?:^|\s)type:(video|audio)/i);
		if (typeMatch) {
			result.type = typeMatch[1].toLowerCase() as MediaType;
			text = text.replace(typeMatch[0], " ");
		}

		// size:<operator><number><unit>
		// e.g., size:<100mb, size:<=10gb
		const sizeMatch = text.match(/(?:^|\s)size:\s*(<=|>=|<|>|=)?\s*(\d+(?:\.\d+)?)(b|kb|mb|gb)/i);
		if (sizeMatch) {
			const op = (sizeMatch[1] || "=") as "<" | "<=" | ">" | ">=" | "=";
			const value = parseFloat(sizeMatch[2]);
			const unit = (sizeMatch[3] || "mb").toLowerCase() as "b" | "kb" | "mb" | "gb";
			result.size = { op, value, unit };
			text = text.replace(sizeMatch[0], " ");
		}

		// status:<value>
		const statusMatch = text.match(/(?:^|\s)status:(success|error|active)/i);
		if (statusMatch) {
			result.status = statusMatch[1].toLowerCase() as StatusType;
			text = text.replace(statusMatch[0], " ");
		}

		// Clean up text (remove extra spaces)
		text = text.replace(/\s{2,}/g, " ").trim();
		if (text) {
			result.text = text;
		}
		return result;
	}

	/**
	 * Filters a list of items using the parsed query.
	 * Items must support at least: type, size(bytes), status and optionally any string content for matching with text search.
	 */
	filterResults<T, TItem extends SearchItem<T>>(items: TItem[], query: string, stringFields: (keyof TItem)[]): TItem[] {
		const filters = this.parse(query);
		logger.debug("filters", { filters });
		return items.filter((item) => {
			// Type filter
			if (filters.type && item.type !== filters.type) return false;
			// Status filter
			if (filters.status && item.status !== filters.status) return false;
			// Size filter
			if (filters.size) {
				const itemSize = item.filesize || 0;
				const filterBytes = this.sizeToBytes(filters.size.value, filters.size.unit);

				switch (filters.size.op) {
					case "<":
						if (!(itemSize < filterBytes)) return false;
						break;
					case "<=":
						if (!(itemSize <= filterBytes)) return false;
						break;
					case ">":
						if (!(itemSize > filterBytes)) return false;
						break;
					case ">=":
						if (!(itemSize >= filterBytes)) return false;
						break;
					case "=":
						if (itemSize !== filterBytes) return false;
						break;
				}
			}
			// Text search (simple: substring match on given string fields)
			if (filters.text) {
				const text = filters.text.toLowerCase();
				const matches = stringFields.find((field) => {
					if (typeof item[field] === "string" && item[field]) {
						return item[field].toLowerCase().includes(text);
					}
					return false;
				});
				if (!matches) return false;
			}
			return true;
		});
	}

	/** Converts a value+unit to bytes */
	private sizeToBytes(value: number, unit: "b" | "kb" | "mb" | "gb"): number {
		switch (unit) {
			case "b":
				return value;
			case "kb":
				return value * 1024;
			case "mb":
				return value * 1024 * 1024;
			case "gb":
				return value * 1024 * 1024 * 1024;
			default:
				return value;
		}
	}
}
