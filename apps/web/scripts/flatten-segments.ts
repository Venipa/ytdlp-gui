// Next 16.4 static export writes segment prefetch files nested (`__next.docs/$oc$slug/__PAGE__.txt`)
// while its client asks for them dot-joined (`__next.docs.$oc$slug.__PAGE__.txt`), so every prefetch 404s.
// shortcut: copies each nested file to the dot-joined name; drop this once Next writes matching names.
import { copyFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const out = join(import.meta.dir, '..', 'out');
let copied = 0;

function walk(dir: string) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (!statSync(p).isDirectory()) continue;
    if (name.startsWith('__next.')) flatten(p, dir);
    else if (name !== '_next') walk(p);
  }
}

function flatten(segDir: string, routeDir: string) {
  const files = (d: string): string[] =>
    readdirSync(d).flatMap((n) => (statSync(join(d, n)).isDirectory() ? files(join(d, n)) : [join(d, n)]));
  for (const f of files(segDir)) {
    copyFileSync(f, join(routeDir, relative(routeDir, f).split(sep).join('.')));
    copied++;
  }
}

walk(out);
console.log(`flatten-segments: ${copied} files`);
