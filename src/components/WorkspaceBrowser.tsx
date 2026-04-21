import { useState, useRef, useCallback, useMemo, ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import Editor, { OnMount } from '@monaco-editor/react';
import {
  File,
  Folder,
  FolderOpen as FolderOpenIcon,
  ChevronRight,
  ChevronDown,
  Save,
  Plus,
  Trash2,
  FolderOpen,
  Copy,
  Check,
  Search,
  X,
  Pencil,
  FolderPlus,
  FilePlus,
} from 'lucide-react';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { confirm } from '@tauri-apps/plugin-dialog';
import { FileEntry } from '../types';
import { getLanguageFromPath } from '../lib/fileLanguage';
import { Input } from './ui';
import { toast } from '../store/toastStore';
import { PluginSurfaceActionBar } from './plugins/PluginSurfaceActionBar';

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

type PendingCreate = { kind: 'file' | 'dir'; parentPath: string } | null;

const ROOT_PATH = '.';
const INDENT_PX = 12;

function joinPath(base: string, name: string): string {
  return base === ROOT_PATH ? name : `${base}/${name}`;
}

function parentOf(path: string): string {
  if (path === ROOT_PATH) return ROOT_PATH;
  const idx = path.lastIndexOf('/');
  return idx === -1 ? ROOT_PATH : path.slice(0, idx);
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface TreeContext {
  adapter: WorkspaceAdapter;
  expanded: Set<string>;
  toggleExpanded: (path: string) => void;
  selectedPath: string;
  setSelectedPath: (path: string) => void;
  editingFile: string | null;
  openFile: (path: string) => void;
  isSpecial: (name: string) => boolean;
  filter: string;
  creating: PendingCreate;
  setCreating: (c: PendingCreate) => void;
  newEntryName: string;
  setNewEntryName: (name: string) => void;
  handleCreate: () => void;
  cancelCreate: () => void;
  renamingPath: string | null;
  setRenamingPath: (path: string | null) => void;
  renameValue: string;
  setRenameValue: (value: string) => void;
  handleRename: (parentPath: string, entry: FileEntry) => void;
  handleDelete: (parentPath: string, entry: FileEntry) => void;
  startCreateIn: (parentPath: string, kind: 'file' | 'dir') => void;
}

function FolderContents({ path, depth, ctx }: { path: string; depth: number; ctx: TreeContext }) {
  const { adapter, filter, creating } = ctx;

  const { data: files = [], isLoading } = useQuery({
    queryKey: [...adapter.queryKey, 'files', path],
    queryFn: () => adapter.listFiles(path),
    refetchInterval: 10_000,
  });

  const filtered = useMemo(() => {
    if (!filter.trim()) return files;
    const q = filter.toLowerCase();
    return files.filter((f) => f.isDir || f.name.toLowerCase().includes(q));
  }, [files, filter]);

  const indentStyle = { paddingLeft: depth * INDENT_PX + 8 };

  if (isLoading && files.length === 0) {
    return (
      <div className="text-muted text-[11px] py-1" style={indentStyle}>
        Loading…
      </div>
    );
  }

  const isCreatingHere = creating && creating.parentPath === path;

  return (
    <>
      {isCreatingHere && <CreateRow depth={depth} ctx={ctx} />}
      {filtered.map((entry) =>
        entry.isDir ? (
          <FolderNode
            key={entry.name}
            parentPath={path}
            entry={entry}
            depth={depth}
            ctx={ctx}
          />
        ) : (
          <FileNode
            key={entry.name}
            parentPath={path}
            entry={entry}
            depth={depth}
            ctx={ctx}
          />
        )
      )}
      {!isLoading && filtered.length === 0 && !isCreatingHere && depth === 0 && (
        <div className="flex flex-col items-center justify-center py-8 gap-2 text-muted text-xs">
          <FolderOpen size={22} className="opacity-40" />
          <span>{filter ? 'No files match your filter' : 'Empty workspace'}</span>
        </div>
      )}
    </>
  );
}

function CreateRow({ depth, ctx }: { depth: number; ctx: TreeContext }) {
  const { creating, newEntryName, setNewEntryName, handleCreate, cancelCreate } = ctx;
  if (!creating) return null;
  return (
    <div
      className="flex items-center gap-1.5 py-1 pr-2"
      style={{ paddingLeft: (depth + 1) * INDENT_PX + 8 }}
    >
      {creating.kind === 'dir' ? (
        <Folder size={13} className="text-accent-hover shrink-0" />
      ) : (
        <File size={13} className="text-muted shrink-0" />
      )}
      <Input
        placeholder={creating.kind === 'dir' ? 'folder-name' : 'filename.md'}
        value={newEntryName}
        onChange={(e) => setNewEntryName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') handleCreate();
          if (e.key === 'Escape') cancelCreate();
        }}
        onBlur={() => {
          if (!newEntryName.trim()) cancelCreate();
        }}
        autoFocus
        aria-label={creating.kind === 'dir' ? 'New folder name' : 'New file name'}
        className="flex-1 bg-background border-accent rounded px-2 py-1 text-xs"
      />
    </div>
  );
}

function FolderNode({
  parentPath,
  entry,
  depth,
  ctx,
}: {
  parentPath: string;
  entry: FileEntry;
  depth: number;
  ctx: TreeContext;
}) {
  const {
    adapter,
    expanded,
    toggleExpanded,
    selectedPath,
    setSelectedPath,
    renamingPath,
    setRenamingPath,
    renameValue,
    setRenameValue,
    handleRename,
    handleDelete,
    startCreateIn,
  } = ctx;

  const path = joinPath(parentPath, entry.name);
  const isOpen = expanded.has(path);
  const isSelected = selectedPath === path;
  const isRenaming = renamingPath === path;

  return (
    <>
      <div
        role="treeitem"
        aria-expanded={isOpen}
        aria-selected={isSelected}
        className={`flex items-center gap-1 py-1 pr-2 cursor-pointer group ${
          isSelected ? 'bg-accent/15 text-white' : 'text-secondary hover:bg-surface hover:text-white'
        }`}
        style={{ paddingLeft: depth * INDENT_PX + 4 }}
        onClick={() => {
          if (isRenaming) return;
          setSelectedPath(path);
          toggleExpanded(path);
        }}
        onDoubleClick={() => {
          if (!adapter.renameEntry || isRenaming) return;
          setRenamingPath(path);
          setRenameValue(entry.name);
        }}
      >
        {isOpen ? (
          <ChevronDown size={12} className="text-muted shrink-0" />
        ) : (
          <ChevronRight size={12} className="text-muted shrink-0" />
        )}
        {isOpen ? (
          <FolderOpenIcon size={13} className="text-accent-hover shrink-0" />
        ) : (
          <Folder size={13} className="text-accent-hover shrink-0" />
        )}
        {isRenaming ? (
          <Input
            autoFocus
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === 'Enter') handleRename(parentPath, entry);
              if (e.key === 'Escape') setRenamingPath(null);
            }}
            onBlur={() => handleRename(parentPath, entry)}
            onClick={(e) => e.stopPropagation()}
            aria-label="New name"
            className="flex-1 bg-background border-accent rounded px-1.5 py-0.5 text-xs font-mono"
          />
        ) : (
          <>
            <span className="text-xs truncate flex-1 font-mono">{entry.name}</span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                if (!isOpen) toggleExpanded(path);
                startCreateIn(path, 'file');
              }}
              aria-label={`New file in ${entry.name}`}
              title="New file"
              className="hidden group-hover:block p-0.5 rounded text-muted hover:text-accent-hover"
            >
              <FilePlus size={11} />
            </button>
            {adapter.createDir && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  if (!isOpen) toggleExpanded(path);
                  startCreateIn(path, 'dir');
                }}
                aria-label={`New folder in ${entry.name}`}
                title="New folder"
                className="hidden group-hover:block p-0.5 rounded text-muted hover:text-accent-hover"
              >
                <FolderPlus size={11} />
              </button>
            )}
            {adapter.renameEntry && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setRenamingPath(path);
                  setRenameValue(entry.name);
                }}
                aria-label={`Rename ${entry.name}`}
                title="Rename"
                className="hidden group-hover:block p-0.5 rounded text-muted hover:text-white"
              >
                <Pencil size={10} />
              </button>
            )}
            <button
              onClick={(e) => {
                e.stopPropagation();
                handleDelete(parentPath, entry);
              }}
              aria-label={`Delete ${entry.name}`}
              title="Delete"
              className="hidden group-hover:block p-0.5 rounded text-muted hover:text-red-400"
            >
              <Trash2 size={10} />
            </button>
          </>
        )}
      </div>
      {isOpen && <FolderContents path={path} depth={depth + 1} ctx={ctx} />}
    </>
  );
}

function FileNode({
  parentPath,
  entry,
  depth,
  ctx,
}: {
  parentPath: string;
  entry: FileEntry;
  depth: number;
  ctx: TreeContext;
}) {
  const {
    adapter,
    editingFile,
    openFile,
    isSpecial,
    renamingPath,
    setRenamingPath,
    renameValue,
    setRenameValue,
    handleRename,
    handleDelete,
  } = ctx;

  const path = joinPath(parentPath, entry.name);
  const isActive = editingFile === path;
  const protectedFile = isSpecial(entry.name);
  const isRenaming = renamingPath === path;

  return (
    <div
      role="treeitem"
      aria-selected={isActive}
      aria-current={isActive ? 'page' : undefined}
      className={`flex items-center gap-1 py-1 pr-2 cursor-pointer group ${
        isActive ? 'bg-accent/15 text-white' : 'text-secondary hover:bg-surface hover:text-white'
      }`}
      style={{ paddingLeft: depth * INDENT_PX + 20 }}
      onClick={() => {
        if (!isRenaming) openFile(path);
      }}
      onDoubleClick={() => {
        if (!adapter.renameEntry || protectedFile || isRenaming) return;
        setRenamingPath(path);
        setRenameValue(entry.name);
      }}
    >
      <File
        size={13}
        className={`shrink-0 ${protectedFile ? 'text-amber-400' : 'text-muted'}`}
      />
      {isRenaming ? (
        <Input
          autoFocus
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') handleRename(parentPath, entry);
            if (e.key === 'Escape') setRenamingPath(null);
          }}
          onBlur={() => handleRename(parentPath, entry)}
          onClick={(e) => e.stopPropagation()}
          aria-label="New name"
          className="flex-1 bg-background border-accent rounded px-1.5 py-0.5 text-xs font-mono"
        />
      ) : (
        <>
          <span className="text-xs truncate flex-1 font-mono">{entry.name}</span>
          <span className="text-[10px] text-muted shrink-0 opacity-0 group-hover:opacity-100">
            {formatSize(entry.sizeBytes)}
          </span>
          {adapter.renameEntry && !protectedFile && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                setRenamingPath(path);
                setRenameValue(entry.name);
              }}
              aria-label={`Rename ${entry.name}`}
              title="Rename"
              className="hidden group-hover:block p-0.5 rounded text-muted hover:text-white"
            >
              <Pencil size={10} />
            </button>
          )}
          {protectedFile ? (
            <button
              disabled
              aria-label={`${entry.name} is a system file and cannot be deleted`}
              title="System file — cannot be deleted"
              className="hidden group-hover:block p-0.5 rounded text-muted/40 cursor-not-allowed"
            >
              <Trash2 size={10} />
            </button>
          ) : (
            <button
              onClick={(e) => {
                e.stopPropagation();
                handleDelete(parentPath, entry);
              }}
              aria-label={`Delete ${entry.name}`}
              title="Delete"
              className="hidden group-hover:block p-0.5 rounded text-muted hover:text-red-400"
            >
              <Trash2 size={10} />
            </button>
          )}
        </>
      )}
    </div>
  );
}

export function WorkspaceBrowser({
  adapter,
  specialFiles = [],
  extraToolbarItems,
}: WorkspaceBrowserProps) {
  const queryClient = useQueryClient();
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([ROOT_PATH]));
  const [selectedPath, setSelectedPath] = useState<string>(ROOT_PATH);
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

  const specialSet = useMemo(() => new Set(specialFiles), [specialFiles]);
  const isSpecial = useCallback((name: string) => specialSet.has(name), [specialSet]);
  const isDirty = editingFile !== null && liveContent !== savedContent;

  const { data: workspacePath } = useQuery({
    queryKey: [...adapter.queryKey, 'path'],
    queryFn: () => adapter.getPath!(),
    enabled: !!adapter.getPath,
  });

  const invalidateFiles = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: [...adapter.queryKey, 'files'] });
  }, [queryClient, adapter.queryKey]);

  const toggleExpanded = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const expandPath = useCallback((path: string) => {
    setExpanded((prev) => {
      if (prev.has(path)) return prev;
      const next = new Set(prev);
      next.add(path);
      return next;
    });
  }, []);

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
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => handleSave());
    },
    [handleSave]
  );

  const confirmDiscardIfDirty = useCallback(async (): Promise<boolean> => {
    if (!isDirty) return true;
    return confirm('Discard unsaved changes?', { title: 'Unsaved changes', kind: 'warning' });
  }, [isDirty]);

  const openFile = useCallback(
    async (path: string) => {
      if (editingFile === path) return;
      if (!(await confirmDiscardIfDirty())) return;
      try {
        const content = await adapter.readFile(path);
        setEditingFile(path);
        setSavedContent(content);
        setLiveContent(content);
        setSelectedPath(parentOf(path));
      } catch (err) {
        toast.error('Failed to read file', err);
      }
    },
    [adapter, confirmDiscardIfDirty, editingFile]
  );

  const handleDelete = useCallback(
    async (parentPath: string, entry: FileEntry) => {
      if (!entry.isDir && isSpecial(entry.name)) return;
      const path = joinPath(parentPath, entry.name);
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
    },
    [adapter, editingFile, invalidateFiles, isSpecial]
  );

  const cancelCreate = useCallback(() => {
    setCreating(null);
    setNewEntryName('');
  }, []);

  const startCreateIn = useCallback(
    (parentPath: string, kind: 'file' | 'dir') => {
      setCreating({ kind, parentPath });
      setNewEntryName('');
      setSelectedPath(parentPath);
      expandPath(parentPath);
    },
    [expandPath]
  );

  const handleCreate = useCallback(async () => {
    if (!creating) return;
    const name = newEntryName.trim();
    if (!name) return;
    const path = joinPath(creating.parentPath, name);
    try {
      if (creating.kind === 'dir') {
        if (!adapter.createDir) {
          toast.error('Creating folders is not supported here');
          return;
        }
        await adapter.createDir(path);
        toast.success(`Created ${name}/`);
        expandPath(path);
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
  }, [adapter, creating, expandPath, invalidateFiles, newEntryName]);

  const handleRename = useCallback(
    async (parentPath: string, entry: FileEntry) => {
      if (!adapter.renameEntry) return;
      const to = renameValue.trim();
      if (!to || to === entry.name) {
        setRenamingPath(null);
        return;
      }
      const from = joinPath(parentPath, entry.name);
      const toPath = joinPath(parentPath, to);
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
    },
    [adapter, editingFile, invalidateFiles, renameValue]
  );

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

  const selectedAbsolutePath = useMemo(() => {
    if (!workspacePath) return null;
    return selectedPath === ROOT_PATH ? workspacePath : `${workspacePath}/${selectedPath}`;
  }, [selectedPath, workspacePath]);

  const ctx: TreeContext = {
    adapter,
    expanded,
    toggleExpanded,
    selectedPath,
    setSelectedPath,
    editingFile,
    openFile,
    isSpecial,
    filter,
    creating,
    setCreating,
    newEntryName,
    setNewEntryName,
    handleCreate,
    cancelCreate,
    renamingPath,
    setRenamingPath,
    renameValue,
    setRenameValue,
    handleRename,
    handleDelete,
    startCreateIn,
  };

  return (
    <div className="flex flex-col h-full">
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
        <div className="w-[260px] flex flex-col border-r border-edge">
          <div className="flex items-center gap-1 px-2 py-2 border-b border-edge min-h-[36px]">
            <span className="flex-1 text-[11px] text-muted font-mono truncate px-1">
              Workspace
            </span>
            {selectedAbsolutePath ? (
              <PluginSurfaceActionBar
                surface="workspaceBrowser"
                path={selectedAbsolutePath}
                variant="workspace"
                maxInlineActions={2}
                onActionComplete={invalidateFiles}
              />
            ) : null}
            {adapter.createDir && (
              <button
                onClick={() => startCreateIn(selectedPath, 'dir')}
                aria-label="New folder"
                title={`New folder in ${selectedPath === ROOT_PATH ? 'workspace' : selectedPath}`}
                className="p-1 rounded text-muted hover:text-accent-hover hover:bg-accent/10 shrink-0"
              >
                <FolderPlus size={13} />
              </button>
            )}
            <button
              onClick={() => startCreateIn(selectedPath, 'file')}
              aria-label="New file"
              title={`New file in ${selectedPath === ROOT_PATH ? 'workspace' : selectedPath}`}
              className="p-1 rounded text-muted hover:text-accent-hover hover:bg-accent/10 shrink-0"
            >
              <Plus size={13} />
            </button>
          </div>

          <div className="flex items-center gap-1.5 px-2 py-1.5 border-b border-edge">
            <Search size={11} className="text-muted shrink-0" />
            <Input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter…"
              aria-label="Filter files"
              className="flex-1 bg-transparent border-transparent rounded px-1 py-0.5 text-[11px] placeholder-muted"
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

          <div
            className="flex-1 overflow-y-auto py-1"
            role="tree"
            aria-label="Workspace files"
          >
            <FolderContents path={ROOT_PATH} depth={0} ctx={ctx} />
          </div>
        </div>

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
                  path={editingFile}
                  language={getLanguageFromPath(editingFile)}
                  theme="vs-dark"
                  value={liveContent}
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
