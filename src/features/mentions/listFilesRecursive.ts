import { workspaceApi } from '../../api/workspace';

const IGNORE = new Set(['.git', 'node_modules', 'target', 'dist', 'build', '.next']);
const MAX_ENTRIES = 2000;

export interface RecursiveFileEntry {
  path: string;
  name: string;
}

export interface RecursiveFilesResult {
  entries: RecursiveFileEntry[];
  truncated: boolean;
}

export async function listFilesRecursive(agentId: string): Promise<RecursiveFilesResult> {
  const entries: RecursiveFileEntry[] = [];
  const queue: string[] = [''];
  let truncated = false;

  while (queue.length > 0) {
    if (entries.length >= MAX_ENTRIES) {
      truncated = true;
      break;
    }
    const dir = queue.shift()!;
    let children;
    try {
      children = await workspaceApi.listFiles(agentId, dir || undefined);
    } catch {
      continue;
    }
    for (const child of children) {
      if (IGNORE.has(child.name)) continue;
      const path = dir ? `${dir}/${child.name}` : child.name;
      if (child.isDir) {
        queue.push(path);
      } else {
        if (entries.length >= MAX_ENTRIES) {
          truncated = true;
          break;
        }
        entries.push({ path, name: child.name });
      }
    }
  }

  return { entries, truncated };
}
