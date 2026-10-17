import { mkdir, writeFile } from 'node:fs/promises';

// Original geometric artwork. All letterforms are paths, with no font dependency.
const dir = new URL('../site/assets/brand/', import.meta.url);
await mkdir(dir, { recursive: true });
const symbol = `<path d="M100 99a50 50 0 1 1 0-70" fill="none" stroke="#2589ed" stroke-width="10" stroke-linecap="round"/>
<path d="m58 61 36-41 11-1v11L75 67l8 34-9 9-16-29-16 13-9-3 12-21-23-15 8-8z" fill="#2589ed"/>
<path d="m94 20 11-1v11L75 67l-8-8z" fill="#39c5ed"/>`;
const letters = `<g fill="none" stroke="currentColor" stroke-width="7" stroke-linecap="round" stroke-linejoin="round">
<path d="M167 50a17 19 0 1 0 0 38 17 19 0 1 0 0-38z"/>
<path d="M201 106V50m0 19a17 19 0 1 1 34 0 17 19 0 1 1-34 0"/>
<path d="M252 69h34c0-26-34-26-34 0 0 19 20 24 32 12"/>
<path d="M303 88V50m0 17c0-24 32-24 32 0v21"/>
</g>
<g fill="none" stroke="currentColor" stroke-width="9" stroke-linecap="square" stroke-linejoin="miter">
<path d="M362 88V34h32m-32 25h27"/>
<path d="M445 39c-8-8-34-9-34 6 0 19 36 11 36 30 0 17-28 17-38 8"/>
</g>
<path d="m465 30 46 62h-15l-46-62z" fill="currentColor"/>
<path d="m497 30-47 62h15l47-62z" fill="#2589ed"/>
<path d="m497 30-17 23h15l17-23z" fill="#39c5ed"/>`;
const svg = (body, viewBox, color, label) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}" role="img" aria-labelledby="title" style="color:${color}"><title id="title">${label}</title>\n${body}\n</svg>\n`;
for (const [name, color] of [['light', '#182f4c'], ['dark', '#eef6ff'], ['mono', '#182f4c']]) {
  const body = name === 'mono' ? (symbol + letters).replaceAll('#2589ed', 'currentColor').replaceAll('#39c5ed', 'currentColor') : symbol + letters;
  await writeFile(new URL(`openfsx-${name}.svg`, dir), svg(body, '0 0 528 128', color, 'openFSX'));
}
await writeFile(new URL('openfsx-icon.svg', dir), svg(symbol, '0 0 128 128', '#182f4c', 'openFSX flight emblem'));
await writeFile(new URL('../site/favicon.svg', import.meta.url), svg(`<rect width="128" height="128" rx="28" fill="#142d4d"/>${symbol.replaceAll('#2589ed', '#f1f7ff')}`, '0 0 128 128', '#fff', 'openFSX'));
console.log('Generated original openFSX SVG brand assets.');
