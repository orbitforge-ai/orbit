import { Plugin } from '@orbit/plugin-sdk';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
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

async function git(args, cwd) {
  return execFileAsync('git', args, { cwd });
}

async function resolveRepoRoot(path) {
  if (!path || typeof path !== 'string') return null;
  try {
    const { stdout } = await git(['rev-parse', '--show-toplevel'], path);
    return stdout.trim() || null;
  } catch {
    return null;
  }
}

async function resolveDefaultBranch(repoRoot) {
  try {
    const { stdout } = await git(
      ['symbolic-ref', '--quiet', '--short', 'refs/remotes/origin/HEAD'],
      repoRoot
    );
    const ref = stdout.trim();
    if (ref.startsWith('origin/')) return ref.slice('origin/'.length);
  } catch {}

  for (const branch of ['main', 'master']) {
    try {
      await git(['rev-parse', '--verify', branch], repoRoot);
      return branch;
    } catch {}
  }
  return null;
}

function requireRepoRoot(input) {
  const repoRoot = input?.context?.target?.token;
  if (!repoRoot || typeof repoRoot !== 'string') {
    throw new Error('Missing repo target for GitHub sidebar action');
  }
  return repoRoot;
}

plugin.tool('resolve_surface_actions', {
  description: 'Return sidebar actions for a repo at the current path.',
  inputSchema: {
    type: 'object',
    properties: {
      surface: { type: 'string' },
      path: { type: ['string', 'null'] },
    },
  },
  run: async ({ input }) => {
    const repoRoot = await resolveRepoRoot(input?.path);
    if (!repoRoot) return { actions: [] };

    const repoName = repoRoot.split('/').filter(Boolean).pop() ?? repoRoot;
    const target = {
      kind: 'gitRepo',
      token: repoRoot,
      displayPath: repoRoot,
    };
    const items = [
      {
        id: 'pull',
        label: 'Pull',
        target,
        tool: 'git_pull',
      },
      {
        id: 'push',
        label: 'Push',
        target,
        tool: 'git_push',
      },
    ];

    const defaultBranch = await resolveDefaultBranch(repoRoot);
    if (defaultBranch) {
      items.push({
        id: `checkout-${defaultBranch}`,
        label: `Checkout ${defaultBranch}`,
        target,
        tool: 'git_checkout_branch',
        args: { branch: defaultBranch },
      });
    }

    return {
      actions: [
        {
          id: 'repo-actions',
          presentation: 'menu',
          label: 'GitHub',
          tooltip: `GitHub actions for ${repoName}`,
          items,
        },
      ],
    };
  },
});

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
    await execFileAsync('git', ['clone', url, dest], { cwd: process.cwd() });
    return { clonedInto: dest };
  },
});

plugin.tool('git_pull', {
  description: 'Run git pull --ff-only for the resolved repo target.',
  inputSchema: {
    type: 'object',
    properties: {
      context: { type: 'object' },
    },
  },
  run: async ({ input }) => {
    const repoRoot = requireRepoRoot(input);
    const { stdout, stderr } = await git(['pull', '--ff-only'], repoRoot);
    return {
      repoRoot,
      stdout: stdout.trim(),
      stderr: stderr.trim(),
    };
  },
});

plugin.tool('git_push', {
  description: 'Run git push for the resolved repo target.',
  inputSchema: {
    type: 'object',
    properties: {
      context: { type: 'object' },
    },
  },
  run: async ({ input }) => {
    const repoRoot = requireRepoRoot(input);
    const { stdout, stderr } = await git(['push'], repoRoot);
    return {
      repoRoot,
      stdout: stdout.trim(),
      stderr: stderr.trim(),
    };
  },
});

plugin.tool('git_checkout_branch', {
  description: 'Check out a branch in the resolved repo target.',
  inputSchema: {
    type: 'object',
    required: ['branch'],
    properties: {
      branch: { type: 'string' },
      context: { type: 'object' },
    },
  },
  run: async ({ input }) => {
    const repoRoot = requireRepoRoot(input);
    if (!input.branch || typeof input.branch !== 'string') {
      throw new Error('branch is required');
    }
    const { stdout, stderr } = await git(['checkout', input.branch], repoRoot);
    return {
      repoRoot,
      branch: input.branch,
      stdout: stdout.trim(),
      stderr: stderr.trim(),
    };
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
