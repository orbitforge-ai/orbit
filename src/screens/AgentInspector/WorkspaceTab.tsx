import { useMemo } from 'react';
import { Settings } from 'lucide-react';
import { workspaceApi } from '../../api/workspace';
import { useUiStore } from '../../store/uiStore';
import { WorkspaceBrowser, WorkspaceAdapter } from '../../components/WorkspaceBrowser';

const SPECIAL_FILES = ['system_prompt.md', 'config.json', 'pulse.md'];

export function WorkspaceTab({ agentId }: { agentId: string }) {
  const setAgentTab = useUiStore((s) => s.setAgentTab);

  const adapter: WorkspaceAdapter = useMemo(
    () => ({
      queryKey: ['workspace', agentId],
      getPath: () => workspaceApi.getWorkspacePath(agentId),
      listFiles: (path) => workspaceApi.listFiles(agentId, path),
      readFile: (path) => workspaceApi.readFile(agentId, path),
      writeFile: (path, content) => workspaceApi.writeFile(agentId, path, content),
      deleteFile: (path) => workspaceApi.deleteFile(agentId, path),
      createDir: (path) => workspaceApi.createDir(agentId, path),
      renameEntry: (from, to) => workspaceApi.renameEntry(agentId, from, to),
      saveSpecialFile: (path, content) =>
        path === 'system_prompt.md'
          ? workspaceApi.updateSystemPrompt(agentId, content)
          : workspaceApi.writeFile(agentId, path, content),
    }),
    [agentId]
  );

  return (
    <WorkspaceBrowser
      adapter={adapter}
      specialFiles={SPECIAL_FILES}
      extraToolbarItems={
        <button
          onClick={() => setAgentTab('config')}
          aria-label="Edit agent config"
          className="p-1.5 rounded text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
          title="Edit agent config"
        >
          <Settings size={14} />
        </button>
      }
    />
  );
}
