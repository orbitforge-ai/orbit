#!/usr/bin/env node
/**
 * create-orbit-plugin CLI. Scaffolds a new plugin from a template.
 *
 * Usage:
 *   npx create-orbit-plugin <name> [--template=<node-tool-only|node-entity|node-oauth>]
 */

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { templates, type TemplateName } from './templates.js';

const args = process.argv.slice(2);
if (args.length === 0 || args[0] === '-h' || args[0] === '--help') {
  console.log(
    `Usage: npx create-orbit-plugin <name> [--template=<${Object.keys(templates).join('|')}>]`
  );
  process.exit(args.length === 0 ? 1 : 0);
}

const name = args[0]!;
if (!/^[a-z][a-z0-9-]{1,}$/.test(name)) {
  console.error(`invalid plugin name ${name}: use kebab-case alphanumerics`);
  process.exit(2);
}

const templateArg = args.find((a) => a.startsWith('--template='));
const templateName = (templateArg?.split('=')[1] ?? 'node-tool-only') as TemplateName;
const template = templates[templateName];
if (!template) {
  console.error(
    `unknown template: ${templateName}. options: ${Object.keys(templates).join(', ')}`
  );
  process.exit(2);
}

const target = path.resolve(process.cwd(), name);
if (fs.existsSync(target)) {
  console.error(`target directory already exists: ${target}`);
  process.exit(3);
}
fs.mkdirSync(target, { recursive: true });

const ctx = {
  pluginName: name,
  pluginId: `com.${process.env.USER?.toLowerCase().replace(/[^a-z0-9]/g, '') || 'author'}.${name.replace(/-/g, '_')}`,
  humanName: name
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' '),
};

for (const file of template(ctx)) {
  const filePath = path.join(target, file.path);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, file.contents, 'utf8');
}

console.log(`Scaffolded ${templateName} plugin at ${target}`);
console.log('');
console.log('Next steps:');
console.log(`  cd ${name}`);
console.log('  npm install');
console.log('  # enable developer.pluginDevMode in settings.json');
console.log('  # then in Orbit: Plugins screen -> Install from directory -> pick this folder');
