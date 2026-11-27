/** Which links get a bar, and which variant. Pure, so the rules are testable without a DOM. */
import { isHttp } from "./sender";

export interface Box { width: number; height: number }

export const RICH_W = 240, RICH_H = 135, SMALL_MIN = 48, MAX_URL = 2048;

const rich = (b: Box) => b.width >= RICH_W && b.height >= RICH_H;
const small = (b: Box) => b.width >= SMALL_MIN && b.height >= SMALL_MIN;
const noHash = (u: string) => u.split("#")[0];

/**
 * `href` is the resolved link target, `page` the frame's own URL. Rich (the player-sized bar) when the
 * thumbnail or else the link is 240x135 or more; small (an icon button) when the thumbnail, else the link,
 * is at least 48x48; otherwise nothing. `thumb` tells whether the bar attaches to the thumbnail.
 */
export function linkTarget(href: string, page: string, link: Box, thumb?: Box): { kind: "rich" | "small"; thumb: boolean } | null {
  if (!isHttp(href) || href.length > MAX_URL || noHash(href) === noHash(page)) return null;
  if (thumb && rich(thumb)) return { kind: "rich", thumb: true };
  if (rich(link)) return { kind: "rich", thumb: false };
  const t = thumb ?? link;
  return small(t) ? { kind: "small", thumb: !!thumb } : null;
}
