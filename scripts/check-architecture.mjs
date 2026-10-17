import { spawnSync } from 'node:child_process';
import { access, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const rules = JSON.parse(await readFile(new URL('./architecture.json', import.meta.url), 'utf8'));
const result = spawnSync('cargo', ['metadata', '--offline', '--locked', '--no-deps', '--format-version', '1'], { cwd: root, encoding: 'utf8' });
if (result.status !== 0) throw new Error(result.stderr || 'cargo metadata failed');
const metadata = JSON.parse(result.stdout);
const members = metadata.packages.filter(p => metadata.workspace_members.includes(p.id));
const failures = [];
for (const pkg of members) {
  const rule = rules[pkg.name];
  if (!rule) { failures.push(`Missing architecture rule for ${pkg.name}`); continue; }
  const actual = path.relative(root, path.dirname(pkg.manifest_path)).replaceAll('\\', '/');
  if (actual !== rule.path) failures.push(`${pkg.name}: expected ${rule.path}, found ${actual}`);
  for (const dep of pkg.dependencies) {
    if (dep.name.startsWith('fsx-') && !rule.dependencies.includes(dep.name)) failures.push(`${pkg.name} must not depend on ${dep.name}`);
    if (rule.headless && ['wgpu', 'winit', 'wry'].includes(dep.name)) failures.push(`${pkg.name} must remain independent of ${dep.name}`);
  }
}
for (const name of Object.keys(rules)) if (!members.some(p => p.name === name)) failures.push(`Workspace member missing: ${name}`);
for (const source of ['src/lib.rs', 'src/main.rs', 'src/bin/viewer.rs']) {
  try { await access(path.join(root, source)); failures.push(`Root source has returned: ${source}`); } catch (e) { if (e.code !== 'ENOENT') throw e; }
}
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Architecture verified: ${members.length} subsystem packages, allowed dependency directions and headless content/simulation layers.`);
