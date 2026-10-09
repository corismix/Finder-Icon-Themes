import fs from 'node:fs';
import path from 'node:path';

/** Material Icon Theme adapter: keep package-specific concerns here. */
export async function load(appDirectory) {
  const packageDir = path.join(appDirectory, 'node_modules', 'material-icon-theme');
  if (!fs.existsSync(packageDir)) {
    throw new Error(`Material assets not installed. Run ./setup.command (or npm install) inside ${appDirectory}`);
  }
  const { generateManifest } = await import('material-icon-theme');
  return {
    name: 'material',
    title: 'Material Icon Theme',
    manifest: generateManifest(),
    assetsDir: path.join(packageDir, 'icons'),
  };
}
