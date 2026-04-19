import { useMemo } from 'react';
import { projectsApi } from '../../api/projects';
import { WorkspaceBrowser, WorkspaceAdapter } from '../../components/WorkspaceBrowser';

export function ProjectWorkspaceTab({ projectId }: { projectId: string }) {
  const adapter: WorkspaceAdapter = useMemo(
    () => ({
      queryKey: ['project-workspace', projectId],
      getPath: () => projectsApi.getWorkspacePath(projectId),
      listFiles: (path) => projectsApi.listFiles(projectId, path),
      readFile: (path) => projectsApi.readFile(projectId, path),
      writeFile: (path, content) => projectsApi.writeFile(projectId, path, content),
      deleteFile: (path) => projectsApi.deleteFile(projectId, path),
      createDir: (path) => projectsApi.createDir(projectId, path),
      renameEntry: (from, to) => projectsApi.renameEntry(projectId, from, to),
    }),
    [projectId]
  );

  return <WorkspaceBrowser adapter={adapter} />;
}
