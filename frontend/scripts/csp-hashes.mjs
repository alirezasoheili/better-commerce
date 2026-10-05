// Hash the actual production inline bytes (including Astro's hydration bootstrap).
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';

const scripts = new Set();
const styles = new Set();
async function walk(directory) {
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, item.name);
    if (item.isDirectory()) await walk(path);
    else if (item.name.endsWith('.html')) {
      const html = await readFile(path, 'utf8');
      for (const [tag, hashes] of [['script', scripts], ['style', styles]]) {
        for (const match of html.matchAll(new RegExp(`<${tag}\\b([^>]*)>([\\s\\S]*?)<\\/${tag}>`, 'gi'))) {
          if (tag === 'script' && /\bsrc\s*=/.test(match[1])) continue;
          hashes.add(`sha256-${createHash('sha256').update(match[2]).digest('base64')}`);
        }
      }
    }
  }
}
await walk('dist');
await writeFile('dist/csp-hashes.json', JSON.stringify({ scripts: [...scripts].sort(), styles: [...styles].sort() }));
