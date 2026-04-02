import { useState, useRef, useCallback } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import Editor, { OnMount } from '@monaco-editor/react';
import {
  File,
  Folder,
  ChevronRight,
  Save,
  Plus,
  Trash2,
  ArrowLeft,
  FolderOpen,
} from 'lucide-react';
import { projectsApi } from '../../api/projects';
import { FileEntry } from '../../types';
import { confirm } from '@tauri-apps/plugin-dialog';

function getLanguageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    js: 'javascript', ts: 'typescript', jsx: 'javascript', tsx: 'typescript',
    json: 'json', md: 'markdown', py: 'python', rs: 'rust',
    toml: 'toml', yaml: 'yaml', yml: 'yaml', html: 'html',
    css: 'css', sh: 'shell', bash: 'shell', xml: 'xml', sql: 'sql', txt: 'plaintext',
  };
  return map[ext] ?? 'plaintext';
}

export function ProjectWorkspaceTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [currentPath, setCurrentPath] = useState('.');
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newFileName, setNewFileName] = useState('');
  const editorRef = useRef<any>(null);

  const { data: files = [], isLoading } = useQuery<FileEntry[]>({
    queryKey: ['project-workspace-files', projectId, currentPath],
    queryFn: () => projectsApi.listFiles(projectId, currentPath),
    enabled: !editingFile,
  });

  const openFile = useCallback(
    async (name: string) => {
      const path = currentPath === '.' ? name : `${currentPath}/${name}`;
      const content = await projectsApi.readFile(projectId, path);
      setEditingFile(path);
      setFileContent(content);
    },
    [projectId, currentPath]
  );

  const saveFile = useCallback(async () => {
    if (!editingFile) return;
    setSaving(true);
    try {
      await projectsApi.writeFile(projectId, editingFile, fileContent);
    } finally {
      setSaving(false);
    }
  }, [projectId, editingFile, fileContent]);

  const deleteEntry = useCallback(
    async (name: string, isDir: boolean) => {
      const path = currentPath === '.' ? name : `${currentPath}/${name}`;
      const ok = await confirm(`Delete ${isDir ? 'folder' : 'file'} "${name}"?`, {
        title: 'Confirm delete',
        kind: 'warning',
      });
      if (!ok) return;
      await projectsApi.deleteFile(projectId, path);
      queryClient.invalidateQueries({ queryKey: ['project-workspace-files', projectId] });
    },
    [projectId, currentPath, queryClient]
  );

  const createFile = useCallback(async () => {
    if (!newFileName.trim()) return;
    const path = currentPath === '.' ? newFileName : `${currentPath}/${newFileName}`;
    await projectsApi.writeFile(projectId, path, '');
    setCreating(false);
    setNewFileName('');
    queryClient.invalidateQueries({ queryKey: ['project-workspace-files', projectId] });
    openFile(newFileName);
  }, [projectId, currentPath, newFileName, queryClient, openFile]);

  const handleEditorMount: OnMount = (editor) => {
    editorRef.current = editor;
    editor.addCommand(
      // Ctrl+S / Cmd+S
      ((window as any).monaco?.KeyMod?.CtrlCmd | (window as any).monaco?.KeyCode?.KeyS) as number,
      saveFile
    );
  };

  // Editor view
  if (editingFile) {
    return (
      <div className="flex flex-col h-full">
        <div className="flex items-center gap-3 px-4 py-2.5 border-b border-edge bg-panel">
          <button
            onClick={() => setEditingFile(null)}
            className="flex items-center gap-1.5 text-xs text-muted hover:text-white transition-colors"
          >
            <ArrowLeft size={13} />
            Back
          </button>
          <span className="flex-1 text-xs text-secondary font-mono truncate">{editingFile}</span>
          <button
            onClick={saveFile}
            disabled={saving}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
          >
            <Save size={12} />
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
        <div className="flex-1 min-h-0">
          <Editor
            value={fileContent}
            language={getLanguageFromPath(editingFile)}
            theme="vs-dark"
            onMount={handleEditorMount}
            onChange={(v) => setFileContent(v ?? '')}
            options={{ minimap: { enabled: false }, fontSize: 13, wordWrap: 'on' }}
          />
        </div>
      </div>
    );
  }

  // Breadcrumb parts
  const parts = currentPath === '.' ? [] : currentPath.split('/');

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-edge bg-panel">
        {/* Breadcrumbs */}
        <div className="flex items-center gap-1 flex-1 min-w-0 text-xs text-secondary">
          <button
            onClick={() => setCurrentPath('.')}
            className="hover:text-white transition-colors font-medium"
          >
            workspace
          </button>
          {parts.map((part, i) => (
            <span key={i} className="flex items-center gap-1">
              <ChevronRight size={11} className="text-muted" />
              <button
                onClick={() => setCurrentPath(parts.slice(0, i + 1).join('/'))}
                className="hover:text-white transition-colors"
              >
                {part}
              </button>
            </span>
          ))}
        </div>
        <button
          onClick={() => setCreating(true)}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium text-muted hover:text-white hover:bg-surface transition-colors"
        >
          <Plus size={13} />
          New file
        </button>
      </div>

      {/* New file input */}
      {creating && (
        <div className="flex items-center gap-2 px-4 py-2 border-b border-edge bg-surface">
          <input
            autoFocus
            value={newFileName}
            onChange={(e) => setNewFileName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') createFile();
              if (e.key === 'Escape') { setCreating(false); setNewFileName(''); }
            }}
            placeholder="filename.txt"
            className="flex-1 bg-transparent text-xs text-white placeholder-muted outline-none"
          />
          <button onClick={createFile} className="text-xs text-accent hover:text-accent-hover">Create</button>
          <button onClick={() => { setCreating(false); setNewFileName(''); }} className="text-xs text-muted hover:text-white">Cancel</button>
        </div>
      )}

      {/* File list */}
      <div className="flex-1 overflow-y-auto">
        {isLoading ? (
          <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
        ) : files.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-32 gap-2 text-muted text-sm">
            <FolderOpen size={24} className="opacity-40" />
            <span>Empty workspace</span>
          </div>
        ) : (
          <ul className="py-1">
            {files.map((entry) => (
              <li
                key={entry.name}
                className="group flex items-center gap-2 px-4 py-2 hover:bg-surface transition-colors"
              >
                <button
                  className="flex items-center gap-2.5 flex-1 min-w-0 text-left"
                  onClick={() => {
                    if (entry.isDir) {
                      setCurrentPath(currentPath === '.' ? entry.name : `${currentPath}/${entry.name}`);
                    } else {
                      openFile(entry.name);
                    }
                  }}
                >
                  {entry.isDir ? (
                    <Folder size={14} className="text-accent-hover shrink-0" />
                  ) : (
                    <File size={14} className="text-muted shrink-0" />
                  )}
                  <span className="text-sm text-white truncate">{entry.name}</span>
                  {!entry.isDir && (
                    <span className="text-xs text-muted ml-auto shrink-0">
                      {entry.sizeBytes < 1024
                        ? `${entry.sizeBytes}B`
                        : `${(entry.sizeBytes / 1024).toFixed(1)}KB`}
                    </span>
                  )}
                </button>
                <button
                  onClick={() => deleteEntry(entry.name, entry.isDir)}
                  className="opacity-0 group-hover:opacity-100 p-1 rounded text-muted hover:text-red-400 transition-all"
                >
                  <Trash2 size={13} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
