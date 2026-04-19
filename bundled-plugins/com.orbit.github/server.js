import { Plugin } from '@orbit/plugin-sdk';
import { spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { exec } from 'node:child_process';

const execAsync = promisify(exec);
const plugin = new Plugin({ id: 'com.orbit.github' });

function ghHeaders(oauth) {
  const token = oauth.github?.accessToken;
  if (!token) throw new Error('GitHub not connected (Plugins → GitHub → Connect)');
  return {
    Accept: 'application/vnd.github+json',
    Authorization: `Bearer ${token}`,
    'User-Agent': 'orbit-plugin',
  };
}

plugin.tool('clone_repo', {
  description: 'Clone a GitHub repo into the agent workspace.',
  inputSchema: {
    type: 'object',
    required: ['repo'],
    properties: { repo: { type: 'string' } },
  },
  run: async ({ input, oauth, log }) => {
    const token = oauth.github?.accessToken;
    if (!token) throw new Error('GitHub not connected');
    const url = `https://x-access-token:${token}@github.com/${input.repo}.git`;
    const dest = input.repo.split('/')[1];
    log(`cloning ${input.repo} into ./${dest}`);
    await execAsync(`git clone ${url} ${dest}`, { cwd: process.cwd() });
    return { clonedInto: dest };
  },
});

plugin.tool('create_pr', {
  description: 'Open a pull request on a GitHub repo.',
  inputSchema: {
    type: 'object',
    required: ['repo', 'title', 'head', 'base'],
    properties: {
      repo: { type: 'string' },
      title: { type: 'string' },
      body: { type: 'string' },
      head: { type: 'string' },
      base: { type: 'string' },
    },
  },
  run: async ({ input, oauth }) => {
    const res = await fetch(`https://api.github.com/repos/${input.repo}/pulls`, {
      method: 'POST',
      headers: { ...ghHeaders(oauth), 'Content-Type': 'application/json' },
      body: JSON.stringify({
        title: input.title,
        body: input.body ?? '',
        head: input.head,
        base: input.base,
      }),
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`GitHub ${res.status}: ${text}`);
    }
    const data = await res.json();
    return { number: data.number, url: data.html_url };
  },
});

plugin.run();
