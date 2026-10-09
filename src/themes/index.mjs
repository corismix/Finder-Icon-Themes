/** Register additional themes here as they are implemented. */
const themes = new Map([
  ['material', {
    name: 'material',
    title: 'Material Icon Theme (VS Code)',
    loader: () => import('./material.mjs'),
  }],
]);

export function listThemes() {
  return [...themes.values()].map(({ name, title }) => ({ name, title }));
}

export async function loadTheme(name, appDirectory) {
  const entry = themes.get(name);
  if (!entry) throw new Error(`Unknown theme: ${name}. Run ./finder-icon-themes themes.`);
  const module = await entry.loader();
  return module.load(appDirectory);
}
