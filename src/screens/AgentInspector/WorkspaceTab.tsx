import { useState, useRef, useCallback, useEffect } from 'react';
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
  Copy,
  Check,
  Settings,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { workspaceApi } from '../../api/workspace';
import { FileEntry } from '../../types';
import { useUiStore } from '../../store/uiStore';
import { confirm } from '@tauri-apps/plugin-dialog';

function getLanguageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    js: 'javascript',
    ts: 'typescript',
    jsx: 'javascript',
    tsx: 'typescript',
    json: 'json',
    md: 'markdown',
    py: 'python',
    rs: 'rust',
    toml: 'toml',
    yaml: 'yaml',
    yml: 'yaml',
    html: 'html',
    css: 'css',
    sh: 'shell',
    bash: 'shell',
    xml: 'xml',
    sql: 'sql',
    txt: 'plaintext',
  };
  return map[ext] ?? 'plaintext';
}

export function WorkspaceTab({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const { setAgentTab } = useUiStore();
  const [currentPath, setCurrentPath] = useState('.');
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newFileName, setNewFileName] = useState('');
  const [copied, setCopied] = useState(false);
  const editorRef = useRef<any>(null);
  const monacoRef = useRef<any>(null);

  const { data: workspacePath } = useQuery({
    queryKey: ['workspace-path', agentId],
    queryFn: (): Promise<string> => invoke('get_workspace_path', { agentId }),
  });

  const { data: files = [], isLoading } = useQuery({
    queryKey: ['workspace-files', agentId, currentPath],
    queryFn: () => workspaceApi.listFiles(agentId, currentPath),
    refetchInterval: 10_000,
  });

  const handleEditorMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => handleSave());
  }, []);

  // Swap editor content and language when file changes (avoids full remount)
  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco || !editingFile) return;

    const model = editor.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, getLanguageFromPath(editingFile));
      editor.setValue(fileContent);
      editor.setScrollPosition({ scrollTop: 0 });
    }
  }, [editingFile]);

  async function handleOpenFile(file: FileEntry) {
    if (file.isDir) {
      const newPath = currentPath === '.' ? file.name : `${currentPath}/${file.name}`;
      setCurrentPath(newPath);
      setEditingFile(null);
      return;
    }
    const path = currentPath === '.' ? file.name : `${currentPath}/${file.name}`;
    try {
      const content = await workspaceApi.readFile(agentId, path);
      setEditingFile(path);
      setFileContent(content);
    } catch (err) {
      console.error('Failed to read file:', err);
    }
  }

  async function handleSave() {
    if (!editingFile) return;
    setSaving(true);
    try {
      const content = editorRef.current?.getValue() ?? fileContent;
      if (editingFile === 'system_prompt.md') {
        await workspaceApi.updateSystemPrompt(agentId, content);
      } else {
        await workspaceApi.writeFile(agentId, editingFile, content);
      }
      queryClient.invalidateQueries({ queryKey: ['workspace-files', agentId] });
    } catch (err) {
      console.error('Failed to save file:', err);
    }
    setSaving(false);
  }

  async function handleDelete(file: FileEntry) {
    const path = currentPath === '.' ? file.name : `${currentPath}/${file.name}`;
    if (!(await confirm(`Delete "${file.name}"?`))) return;
    try {
      await workspaceApi.deleteFile(agentId, path);
      if (editingFile === path) setEditingFile(null);
      queryClient.invalidateQueries({ queryKey: ['workspace-files', agentId] });
    } catch (err) {
      console.error('Failed to delete:', err);
    }
  }

  async function handleCreate() {
    if (!newFileName.trim()) return;
    const path = currentPath === '.' ? newFileName.trim() : `${currentPath}/${newFileName.trim()}`;
    try {
      await workspaceApi.writeFile(agentId, path, '');
      setCreating(false);
      setNewFileName('');
      queryClient.invalidateQueries({ queryKey: ['workspace-files', agentId] });
      setEditingFile(path);
      setFileContent('');
    } catch (err) {
      console.error('Failed to create file:', err);
    }
  }

  function navigateUp() {
    if (currentPath === '.') return;
    const parts = currentPath.split('/');
    parts.pop();
    setCurrentPath(parts.length === 0 ? '.' : parts.join('/'));
    setEditingFile(null);
  }

  async function handleCopyPath() {
    if (!workspacePath) return;
    await navigator.clipboard.writeText(workspacePath);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  async function handleOpenInFinder() {
    if (!workspacePath) return;
    try {
      await revealItemInDir(workspacePath);
    } catch (err) {
      console.error('Failed to open folder:', err);
    }
  }

  const isSpecialFile = (name: string) =>
    name === 'system_prompt.md' || name === 'config.json' || name === 'pulse.md';

  return (
    <div className="flex flex-col h-full">
      {/* Path bar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-edge bg-panel">
        <button
          onClick={handleCopyPath}
          className="flex items-center gap-1.5 px-2 py-1 rounded text-xs font-mono text-muted hover:text-white hover:bg-surface transition-colors truncate min-w-0"
          title="Click to copy path"
        >
          {copied ? (
            <Check size={11} className="text-emerald-400 shrink-0" />
          ) : (
            <Copy size={11} className="shrink-0" />
          )}
          <span className="truncate">{workspacePath ?? '...'}</span>
        </button>
        <div className="flex items-center gap-1 shrink-0 ml-auto">
          <button
            onClick={handleOpenInFinder}
            className="p-1.5 rounded text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
            title="Open in Finder"
          >
            <FolderOpen size={14} />
          </button>
          <button
            onClick={() => setAgentTab('config')}
            className="p-1.5 rounded text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
            title="Edit agent config"
          >
            <Settings size={14} />
          </button>
        </div>
      </div>

      <div className="flex flex-1 min-h-0">
        {/* File tree */}
        <div className="w-[240px] flex flex-col border-r border-edge">
          <div className="flex items-center justify-between px-3 py-2 border-b border-edge">
            <div className="flex items-center gap-2 min-w-0">
              {currentPath !== '.' && (
                <button
                  onClick={navigateUp}
                  className="p-1 rounded text-muted hover:text-white hover:bg-edge shrink-0"
                >
                  <ArrowLeft size={13} />
                </button>
              )}
              <span className="text-[10px] text-muted font-mono truncate">
                {currentPath === '.' ? '/' : `/${currentPath}`}
              </span>
            </div>
            <button
              onClick={() => setCreating(true)}
              className="p-1 rounded text-muted hover:text-accent-hover hover:bg-accent/10 shrink-0"
            >
              <Plus size={13} />
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-1.5 space-y-0.5">
            {isLoading && <div className="text-center py-4 text-muted text-xs">Loading...</div>}

            {creating && (
              <div className="flex items-center gap-1 px-2 py-1.5">
                <input
                  type="text"
                  placeholder="filename.md"
                  value={newFileName}
                  onChange={(e) => setNewFileName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleCreate();
                    if (e.key === 'Escape') setCreating(false);
                  }}
                  autoFocus
                  className="flex-1 px-2 py-1 rounded bg-background border border-accent text-white text-xs focus:outline-none"
                />
              </div>
            )}

            {files.map((file) => {
              const filePath = currentPath === '.' ? file.name : `${currentPath}/${file.name}`;
              const isActive = editingFile === filePath;

              return (
                <div
                  key={file.name}
                  className={`flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer group ${
                    isActive
                      ? 'bg-accent/15 text-white'
                      : 'text-secondary hover:bg-surface hover:text-white'
                  }`}
                  onClick={() => handleOpenFile(file)}
                >
                  {file.isDir ? (
                    <Folder size={13} className="text-accent-hover shrink-0" />
                  ) : (
                    <File
                      size={13}
                      className={`shrink-0 ${
                        isSpecialFile(file.name) ? 'text-amber-400' : 'text-muted'
                      }`}
                    />
                  )}
                  <span className="text-xs truncate flex-1 font-mono">{file.name}</span>
                  {file.isDir && <ChevronRight size={11} className="text-muted shrink-0" />}
                  {!file.isDir && !isSpecialFile(file.name) && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDelete(file);
                      }}
                      className="hidden group-hover:block p-0.5 rounded text-muted hover:text-red-400"
                    >
                      <Trash2 size={10} />
                    </button>
                  )}
                </div>
              );
            })}

            {!isLoading && files.length === 0 && (
              <div className="text-center py-4 text-muted text-xs">Empty directory</div>
            )}
          </div>
        </div>

        {/* Monaco editor */}
        <div className="flex-1 flex flex-col min-w-0">
          {editingFile ? (
            <>
              <div className="flex items-center justify-between px-4 py-2 border-b border-edge">
                <span className="text-xs text-secondary font-mono truncate">{editingFile}</span>
                <button
                  onClick={handleSave}
                  disabled={saving}
                  className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium shrink-0"
                >
                  <Save size={11} />
                  {saving ? 'Saving...' : 'Save'}
                </button>
              </div>
              <div className="flex-1 min-h-0">
                <Editor
                  defaultValue=""
                  language={getLanguageFromPath(editingFile)}
                  theme="vs-dark"
                  onMount={handleEditorMount}
                  onChange={(value) => setFileContent(value ?? '')}
                  options={{
                    minimap: { enabled: false },
                    fontSize: 13,
                    lineHeight: 20,
                    padding: { top: 12 },
                    scrollBeyondLastLine: false,
                    wordWrap: 'on',
                    automaticLayout: true,
                    tabSize: 2,
                    renderLineHighlight: 'none',
                    overviewRulerLanes: 0,
                    hideCursorInOverviewRuler: true,
                    scrollbar: {
                      verticalScrollbarSize: 6,
                      horizontalScrollbarSize: 6,
                    },
                  }}
                />
              </div>
            </>
          ) : (
            <div className="flex items-center justify-center h-full text-muted text-sm">
              Select a file to edit
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
