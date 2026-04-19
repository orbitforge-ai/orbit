import { useState, useRef, useCallback, useEffect, useMemo, ReactNode } from 'react';
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
  Search,
  X,
  Pencil,
  FolderPlus,
} from 'lucide-react';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { confirm } from '@tauri-apps/plugin-dialog';
import { FileEntry } from '../types';
import { getLanguageFromPath } from '../lib/fileLanguage';
import { toast } from '../store/toastStore';

export interface WorkspaceAdapter {
  queryKey: readonly unknown[];
  getPath?: () => Promise<string>;
  listFiles: (path: string) => Promise<FileEntry[]>;
  readFile: (path: string) => Promise<string>;
  writeFile: (path: string, content: string) => Promise<void>;
  deleteFile: (path: string) => Promise<void>;
  createDir?: (path: string) => Promise<void>;
  renameEntry?: (from: string, to: string) => Promise<void>;
  saveSpecialFile?: (path: string, content: string) => Promise<void>;
}

export interface WorkspaceBrowserProps {
  adapter: WorkspaceAdapter;
  specialFiles?: string[];
  extraToolbarItems?: ReactNode;
}

type PendingCreate = { kind: 'file' | 'dir' } | null;

function joinPath(base: string, name: string): string {
  return base === '.' ? name : `${base}/${name}`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function WorkspaceBrowser({
  adapter,
  specialFiles = [],
  extraToolbarItems,
}: WorkspaceBrowserProps) {
  const queryClient = useQueryClient();
  const [currentPath, setCurrentPath] = useState('.');
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [savedContent, setSavedContent] = useState('');
  const [liveContent, setLiveContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState<PendingCreate>(null);
  const [newEntryName, setNewEntryName] = useState('');
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [filter, setFilter] = useState('');
  const [copied, setCopied] = useState(false);
  const editorRef = useRef<any>(null);
  const monacoRef = useRef<any>(null);

  const specialSet = useMemo(() => new Set(specialFiles), [specialFiles]);
  const isSpecial = useCallback((name: string) => specialSet.has(name), [specialSet]);
  const isDirty = editingFile !== null && liveContent !== savedContent;

  const { data: workspacePath } = useQuery({
    queryKey: [...adapter.queryKey, 'path'],
    queryFn: () => adapter.getPath!(),
    enabled: !!adapter.getPath,
  });

  const { data: files = [], isLoading } = useQuery({
    queryKey: [...adapter.queryKey, 'files', currentPath],
    queryFn: () => adapter.listFiles(currentPath),
    refetchInterval: 10_000,
  });

  const filtered = useMemo(() => {
    if (!filter.trim()) return files;
    const q = filter.toLowerCase();
    return files.filter((f) => f.name.toLowerCase().includes(q));
  }, [files, filter]);

  const invalidateFiles = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: [...adapter.queryKey, 'files'] });
  }, [queryClient, adapter.queryKey]);

  const handleSave = useCallback(async () => {
    if (!editingFile) return;
    setSaving(true);
    try {
      const content = editorRef.current?.getValue() ?? liveContent;
      if (isSpecial(editingFile) && adapter.saveSpecialFile) {
        await adapter.saveSpecialFile(editingFile, content);
      } else {
        await adapter.writeFile(editingFile, content);
      }
      setSavedContent(content);
      setLiveContent(content);
      invalidateFiles();
      toast.success('Saved');
    } catch (err) {
      toast.error('Failed to save file', err);
    } finally {
      setSaving(false);
    }
  }, [editingFile, liveContent, adapter, isSpecial, invalidateFiles]);

  const handleEditorMount: OnMount = useCallback(
    (editor, monaco) => {
      editorRef.current = editor;
      monacoRef.current = monaco;
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => handleSave());
    },
    [handleSave]
  );

  // Swap editor content / language when switching files without remounting.
  useEffect(() => {
    const editor = editorRef.current;
    const monaco = monacoRef.current;
    if (!editor || !monaco || !editingFile) return;
    const model = editor.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, getLanguageFromPath(editingFile));
      editor.setValue(savedContent);
      editor.setScrollPosition({ scrollTop: 0 });
    }
  }, [editingFile]);

  async function confirmDiscardIfDirty(): Promise<boolean> {
    if (!isDirty) return true;
    return confirm('Discard unsaved changes?', { title: 'Unsaved changes', kind: 'warning' });
  }

  async function handleOpenEntry(entry: FileEntry) {
    if (entry.isDir) {
      if (!(await confirmDiscardIfDirty())) return;
      setCurrentPath(joinPath(currentPath, entry.name));
      setEditingFile(null);
      return;
    }
    const path = joinPath(currentPath, entry.name);
    if (editingFile === path) return;
    if (!(await confirmDiscardIfDirty())) return;
    try {
      const content = await adapter.readFile(path);
      setEditingFile(path);
      setSavedContent(content);
      setLiveContent(content);
    } catch (err) {
      toast.error('Failed to read file', err);
    }
  }

  async function handleDelete(entry: FileEntry) {
    if (isSpecial(entry.name)) return;
    const path = joinPath(currentPath, entry.name);
    const ok = await confirm(`Delete ${entry.isDir ? 'folder' : 'file'} "${entry.name}"?`, {
      title: 'Confirm delete',
      kind: 'warning',
    });
    if (!ok) return;
    try {
      await adapter.deleteFile(path);
      if (editingFile === path) {
        setEditingFile(null);
        setSavedContent('');
        setLiveContent('');
      }
      invalidateFiles();
      toast.success(`Deleted ${entry.name}`);
    } catch (err) {
      toast.error('Failed to delete', err);
    }
  }

  async function handleCreate() {
    const name = newEntryName.trim();
    if (!name || !creating) return;
    const path = joinPath(currentPath, name);
    try {
      if (creating.kind === 'dir') {
        if (!adapter.createDir) {
          toast.error('Creating folders is not supported here');
          return;
        }
        await adapter.createDir(path);
        toast.success(`Created ${name}/`);
      } else {
        await adapter.writeFile(path, '');
        toast.success(`Created ${name}`);
        setEditingFile(path);
        setSavedContent('');
        setLiveContent('');
      }
      setCreating(null);
      setNewEntryName('');
      invalidateFiles();
    } catch (err) {
      toast.error('Failed to create', err);
    }
  }

  async function handleRename(entry: FileEntry) {
    if (!adapter.renameEntry) return;
    const to = renameValue.trim();
    if (!to || to === entry.name) {
      setRenamingPath(null);
      return;
    }
    const from = joinPath(currentPath, entry.name);
    const toPath = joinPath(currentPath, to);
    try {
      await adapter.renameEntry(from, toPath);
      if (editingFile === from) setEditingFile(toPath);
      invalidateFiles();
      toast.success(`Renamed to ${to}`);
    } catch (err) {
      toast.error('Failed to rename', err);
    } finally {
      setRenamingPath(null);
    }
  }

  async function navigateUp() {
    if (currentPath === '.') return;
    if (!(await confirmDiscardIfDirty())) return;
    const parts = currentPath.split('/');
    parts.pop();
    setCurrentPath(parts.length === 0 ? '.' : parts.join('/'));
    setEditingFile(null);
  }

  async function navigateTo(path: string) {
    if (!(await confirmDiscardIfDirty())) return;
    setCurrentPath(path);
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
      toast.error('Failed to open folder', err);
    }
  }

  const parts = currentPath === '.' ? [] : currentPath.split('/');

  return (
    <div className="flex flex-col h-full">
      {/* Path bar */}
      {(workspacePath || extraToolbarItems) && (
        <div className="flex items-center gap-2 px-4 py-2 border-b border-edge bg-panel">
          {workspacePath && (
            <button
              onClick={handleCopyPath}
              aria-label="Copy workspace path"
              className="flex items-center gap-1.5 px-2 py-1 rounded text-xs font-mono text-muted hover:text-white hover:bg-surface transition-colors truncate min-w-0"
              title="Click to copy path"
            >
              {copied ? (
                <Check size={11} className="text-emerald-400 shrink-0" />
              ) : (
                <Copy size={11} className="shrink-0" />
              )}
              <span className="truncate">{workspacePath}</span>
            </button>
          )}
          <div className="flex items-center gap-1 shrink-0 ml-auto">
            {workspacePath && (
              <button
                onClick={handleOpenInFinder}
                aria-label="Reveal workspace in Finder"
                className="p-1.5 rounded text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
                title="Open in Finder"
              >
                <FolderOpen size={14} />
              </button>
            )}
            {extraToolbarItems}
          </div>
        </div>
      )}

      <div className="flex flex-1 min-h-0">
        {/* File tree */}
        <div className="w-[260px] flex flex-col border-r border-edge">
          {/* Tree header: breadcrumbs + actions */}
          <div className="flex items-center gap-1 px-2 py-2 border-b border-edge min-h-[36px]">
            {currentPath !== '.' && (
              <button
                onClick={navigateUp}
                aria-label="Go up one folder"
                className="p-1 rounded text-muted hover:text-white hover:bg-edge shrink-0"
                title="Up"
              >
                <ArrowLeft size={13} />
              </button>
            )}
            <div className="flex items-center gap-0.5 flex-1 min-w-0 text-[11px] text-muted font-mono">
              <button
                onClick={() => navigateTo('.')}
                className="hover:text-white px-1 truncate"
              >
                /
              </button>
              {parts.map((part, i) => (
                <span key={i} className="flex items-center gap-0.5 min-w-0">
                  <ChevronRight size={10} className="shrink-0" />
                  <button
                    onClick={() => navigateTo(parts.slice(0, i + 1).join('/'))}
                    className="hover:text-white px-0.5 truncate"
                  >
                    {part}
                  </button>
                </span>
              ))}
            </div>
            {adapter.createDir && (
              <button
                onClick={() => {
                  setCreating({ kind: 'dir' });
                  setNewEntryName('');
                }}
                aria-label="New folder"
                title="New folder"
                className="p-1 rounded text-muted hover:text-accent-hover hover:bg-accent/10 shrink-0"
              >
                <FolderPlus size={13} />
              </button>
            )}
            <button
              onClick={() => {
                setCreating({ kind: 'file' });
                setNewEntryName('');
              }}
              aria-label="New file"
              title="New file"
              className="p-1 rounded text-muted hover:text-accent-hover hover:bg-accent/10 shrink-0"
            >
              <Plus size={13} />
            </button>
          </div>

          {/* Filter */}
          <div className="flex items-center gap-1.5 px-2 py-1.5 border-b border-edge">
            <Search size={11} className="text-muted shrink-0" />
            <input
              type="text"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter…"
              aria-label="Filter files"
              className="flex-1 bg-transparent text-[11px] text-white placeholder-muted focus:outline-none focus:ring-1 focus:ring-accent/50 rounded px-1 py-0.5"
            />
            {filter && (
              <button
                onClick={() => setFilter('')}
                aria-label="Clear filter"
                className="p-0.5 rounded text-muted hover:text-white"
              >
                <X size={10} />
              </button>
            )}
          </div>

          {/* File list */}
          <div
            className="flex-1 overflow-y-auto p-1.5 space-y-0.5"
            role="listbox"
            aria-label="Workspace files"
          >
            {isLoading && <div className="text-center py-4 text-muted text-xs">Loading…</div>}

            {creating && (
              <div className="flex items-center gap-1.5 px-2 py-1.5">
                {creating.kind === 'dir' ? (
                  <Folder size={13} className="text-accent-hover shrink-0" />
                ) : (
                  <File size={13} className="text-muted shrink-0" />
                )}
                <input
                  type="text"
                  placeholder={creating.kind === 'dir' ? 'folder-name' : 'filename.md'}
                  value={newEntryName}
                  onChange={(e) => setNewEntryName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleCreate();
                    if (e.key === 'Escape') {
                      setCreating(null);
                      setNewEntryName('');
                    }
                  }}
                  onBlur={() => {
                    if (!newEntryName.trim()) {
                      setCreating(null);
                    }
                  }}
                  autoFocus
                  aria-label={creating.kind === 'dir' ? 'New folder name' : 'New file name'}
                  className="flex-1 px-2 py-1 rounded bg-background border border-accent text-white text-xs focus:outline-none focus:ring-1 focus:ring-accent/50"
                />
              </div>
            )}

            {filtered.map((file) => {
              const filePath = joinPath(currentPath, file.name);
              const isActive = editingFile === filePath;
              const protectedFile = !file.isDir && isSpecial(file.name);
              const isRenaming = renamingPath === filePath;

              return (
                <div
                  key={file.name}
                  role="option"
                  aria-selected={isActive}
                  aria-current={isActive ? 'page' : undefined}
                  className={`flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer group ${
                    isActive
                      ? 'bg-accent/15 text-white'
                      : 'text-secondary hover:bg-surface hover:text-white'
                  }`}
                  onClick={() => {
                    if (!isRenaming) handleOpenEntry(file);
                  }}
                  onDoubleClick={() => {
                    if (!adapter.renameEntry || protectedFile || isRenaming) return;
                    setRenamingPath(filePath);
                    setRenameValue(file.name);
                  }}
                >
                  {file.isDir ? (
                    <Folder size={13} className="text-accent-hover shrink-0" />
                  ) : (
                    <File
                      size={13}
                      className={`shrink-0 ${protectedFile ? 'text-amber-400' : 'text-muted'}`}
                    />
                  )}
                  {isRenaming ? (
                    <input
                      autoFocus
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onKeyDown={(e) => {
                        e.stopPropagation();
                        if (e.key === 'Enter') handleRename(file);
                        if (e.key === 'Escape') setRenamingPath(null);
                      }}
                      onBlur={() => handleRename(file)}
                      onClick={(e) => e.stopPropagation()}
                      aria-label="New name"
                      className="flex-1 px-1.5 py-0.5 rounded bg-background border border-accent text-white text-xs font-mono focus:outline-none focus:ring-1 focus:ring-accent/50"
                    />
                  ) : (
                    <>
                      <span className="text-xs truncate flex-1 font-mono">{file.name}</span>
                      {!file.isDir && (
                        <span className="text-[10px] text-muted shrink-0 opacity-0 group-hover:opacity-100">
                          {formatSize(file.sizeBytes)}
                        </span>
                      )}
                      {file.isDir && (
                        <ChevronRight size={11} className="text-muted shrink-0" />
                      )}
                      {adapter.renameEntry && !protectedFile && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setRenamingPath(filePath);
                            setRenameValue(file.name);
                          }}
                          aria-label={`Rename ${file.name}`}
                          className="hidden group-hover:block p-0.5 rounded text-muted hover:text-white"
                          title="Rename"
                        >
                          <Pencil size={10} />
                        </button>
                      )}
                      {protectedFile ? (
                        <button
                          disabled
                          aria-label={`${file.name} is a system file and cannot be deleted`}
                          title="System file — cannot be deleted"
                          className="hidden group-hover:block p-0.5 rounded text-muted/40 cursor-not-allowed"
                        >
                          <Trash2 size={10} />
                        </button>
                      ) : (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDelete(file);
                          }}
                          aria-label={`Delete ${file.name}`}
                          className="hidden group-hover:block p-0.5 rounded text-muted hover:text-red-400"
                          title="Delete"
                        >
                          <Trash2 size={10} />
                        </button>
                      )}
                    </>
                  )}
                </div>
              );
            })}

            {!isLoading && filtered.length === 0 && !creating && (
              <div className="flex flex-col items-center justify-center py-8 gap-2 text-muted text-xs">
                <FolderOpen size={22} className="opacity-40" />
                <span>
                  {filter
                    ? 'No files match your filter'
                    : currentPath === '.'
                      ? 'Empty workspace'
                      : 'Empty folder'}
                </span>
              </div>
            )}
          </div>
        </div>

        {/* Editor pane */}
        <div className="flex-1 flex flex-col min-w-0">
          {editingFile ? (
            <>
              <div className="flex items-center justify-between px-4 py-2 border-b border-edge">
                <span className="flex items-center gap-1.5 text-xs text-secondary font-mono truncate min-w-0">
                  <span className="truncate">{editingFile}</span>
                  {isDirty && (
                    <span
                      aria-label="Unsaved changes"
                      title="Unsaved changes"
                      className="text-accent-hover shrink-0"
                    >
                      •
                    </span>
                  )}
                </span>
                <button
                  onClick={handleSave}
                  disabled={saving || !isDirty}
                  aria-label="Save file"
                  className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed text-white text-xs font-medium shrink-0 transition-colors"
                >
                  <Save size={11} />
                  {saving ? 'Saving…' : 'Save'}
                </button>
              </div>
              <div className="flex-1 min-h-0">
                <Editor
                  defaultValue=""
                  language={getLanguageFromPath(editingFile)}
                  theme="vs-dark"
                  onMount={handleEditorMount}
                  onChange={(value) => setLiveContent(value ?? '')}
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
