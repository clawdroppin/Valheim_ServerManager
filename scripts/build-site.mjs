// Assembles the GitHub Pages site: site/index.html + docs/screenshots + the icon → dist-site/.
// Publish: the contents of dist-site/ are the `gh-pages` branch (see AGENTS.md §0).
import { cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const out = join(root, 'dist-site');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(join(root, 'site', 'index.html'), join(out, 'index.html'));
cpSync(join(root, 'docs', 'screenshots'), join(out, 'screenshots'), { recursive: true });
cpSync(join(root, 'docs', 'icon.png'), join(out, 'icon.png'));
writeFileSync(join(out, '.nojekyll'), '');
console.log(`✓ Site ready in ${out}`);
