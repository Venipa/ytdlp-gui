// Wire protocol v1 shared with the ytdlpd bridge (see docs/plan.md "Browser extension").

export const PROTO = 1;
/** Ports to probe, release app first then the ytdlpd.dev build. */
export const PORTS: number[] = [
  ...Array.from({ length: 10 }, (_, i) => 47821 + i),
  ...Array.from({ length: 10 }, (_, i) => 48821 + i),
];

/** Every error code the app can send, plus the ones raised in the extension. A test checks each has an `err_` string. */
export const ERROR_CODES = [
  "unauthorized", "bad_token", "bad_code", "code_expired", "rate_limited", "version",
  "unknown_type", "bad_request", "bad_url", "too_large", "engine_unavailable", "busy", "internal",
  "offline", "timeout", "not_paired", "unsupported",
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];

export type Kind = "video" | "playlist" | "channel" | "unknown";
export interface PresetItem { id: string; label: string }
export interface Checked {
  url: string;
  supported: boolean;
  extractor: string | null;
  name: string | null;
  kind: Kind;
  presets?: PresetItem[];
  preset?: string;
}
export interface Added { download: number | null; state: "queued" | "intake"; duplicate: boolean }
export interface HelloOk { app: "ytdlpd"; app_version: string; proto: number; instance: string; label: string }

export type Frame = { v: number; id?: number; type: string; [k: string]: unknown };

/** A failure with a code from ERROR_CODES (or a code a newer app sent). */
export class WireError extends Error {
  constructor(public code: string, message = code, public retryAfterMs?: number) {
    super(message);
  }
}

/** The connection state the bar and popup see. */
export type State = "offline" | "unpaired" | "removed" | "ready";

/** Messages content/popup send to the service worker. URLs never travel in them except the popup's own. */
export type Msg =
  | { t: "check"; cover: boolean }
  | { t: "add"; preset: string | null; cover: boolean }
  | { t: "status" }
  | { t: "pair"; code: string }
  | { t: "disconnect" }
  | { t: "host" }
  | { t: "view" }
  | { t: "hide"; scope: "page" | "domain" }
  | { t: "top" }
  | { t: "check-link"; url: string }
  | { t: "add-link"; url: string; preset: string | null }
  | { t: "check-url"; url: string }
  | { t: "add-url"; url: string };

/** Chrome storage keys. */
export interface Hidden { pages: string[]; domains: string[] }
