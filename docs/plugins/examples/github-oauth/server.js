import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: 'com.example.github' });

function ghFetch(oauth, path, init = {}) {
  const token = oauth.github?.accessToken;
  if (!token) throw new Error('GitHub not connected');
  return fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${token}`,
      ...(init.headers ?? {}),
    },
  });
}

plugin.tool('clone_repo', {
  description: 'Clone a GitHub repo via the API (metadata only in this demo).',
  inputSchema: {
    type: 'object',
    required: ['repo'],
    properties: { repo: { type: 'string' } },
  },
  run: async ({ input, oauth }) => {
    const res = await ghFetch(oauth, `/repos/${input.repo}`);
    if (!res.ok) throw new Error(`GitHub ${res.status}`);
    const data = await res.json();
    return { fullName: data.full_name, defaultBranch: data.default_branch };
  },
});

plugin.tool('create_pr', {
  description: 'Open a pull request.',
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
    const res = await ghFetch(oauth, `/repos/${input.repo}/pulls`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        title: input.title,
        body: input.body,
        head: input.head,
        base: input.base,
      }),
    });
    if (!res.ok) throw new Error(`GitHub ${res.status}`);
    const data = await res.json();
    return { number: data.number, url: data.html_url };
  },
});

plugin.run();
