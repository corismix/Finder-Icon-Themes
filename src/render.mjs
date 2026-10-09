import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { Resvg } from '@resvg/resvg-js';

export function pngFor(source, cacheDir, assetsDir) {
  // Manifest contains paths such as ../icons/folder-src.svg; never follow paths outside the package.
  const name = path.basename(source);
  if (!name.endsWith('.svg')) throw new Error(`Unexpected icon resource: ${source}`);
  const svgFile = path.join(assetsDir, name);
  const svg = fs.readFileSync(svgFile);
  const key = crypto.createHash('sha256').update(svg).digest('hex');
  const file = path.join(cacheDir, `${key}.png`);
  if (!fs.existsSync(file)) {
    fs.mkdirSync(cacheDir, { recursive: true, mode: 0o700 });
    const rendered = new Resvg(svg, { fitTo: { mode: 'width', value: 256 } }).render().asPng();
    fs.writeFileSync(file, rendered);
  }
  return file;
}
