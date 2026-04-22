import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '@tauri-apps/plugin-dialog';
import { FolderOpen, HardDrive, History, KanbanSquare, ListChecks, MessageSquare, Pencil, Plus, Trash2, Users, Workflow } from 'lucide-react';
import { projectsApi } from '../../api/projects';
import { Project, ProjectSummary } from '../../types';
import { useUiStore } from '../../store/uiStore';
import { cn } from '../../lib/cn';
import { ProjectWorkspaceTab } from './ProjectWorkspaceTab';
import { ProjectAgentsTab } from './ProjectAgentsTab';
import { ProjectBoardTab } from './ProjectBoardTab';
import { ProjectChatTab } from './ProjectChatTab';
import { ProjectScheduledTab } from './ProjectScheduledTab';
import { ProjectWorkflowsTab } from './ProjectWorkflowsTab';
import { ProjectHistoryTab } from './ProjectHistoryTab';
import { Input, SimpleSelect } from '../../components/ui';
import { WorkspacePathChip } from '../../components/WorkspacePathChip';

const TABS = [
  { id: 'workspace' as const, label: 'Workspace', icon: HardDrive },
  { id: 'agents' as const, label: 'Agents', icon: Users },
  { id: 'chat' as const, label: 'Chat', icon: MessageSquare },
  { id: 'board' as const, label: 'Board', icon: KanbanSquare },
  { id: 'scheduled' as const, label: 'Scheduled', icon: ListChecks },
  { id: 'workflows' as const, label: 'Workflows', icon: Workflow },
  { id: 'history' as const, label: 'History', icon: History },
];

export function ProjectInspector() {
  const { selectedProjectId, selectProject } = useUiStore();
  const queryClient = useQueryClient();

  const { data: projects = [] } = useQuery<ProjectSummary[]>({
    queryKey: ['projects'],
    queryFn: projectsApi.list,
  });

  // Listen for the "new project" event dispatched from the Sidebar
  const [showCreate, setShowCreate] = useState(false);
  useEffect(() => {
    const handler = () => setShowCreate(true);
    window.addEventListener('orbit:new-project', handler);
    return () => window.removeEventListener('orbit:new-project', handler);
  }, []);

  const selectedProject = projects.find((p) => p.id === selectedProjectId) ?? null;

  if (selectedProject) {
    return (
      <ProjectDetail
        key={selectedProject.id}
        project={selectedProject}
        onDeleted={() => {
          selectProject(null);
          queryClient.invalidateQueries({ queryKey: ['projects'] });
        }}
      />
    );
  }

  return (
    <ProjectList
      projects={projects}
      showCreate={showCreate}
      onCreateOpen={() => setShowCreate(true)}
      onCreateClose={() => setShowCreate(false)}
      onCreated={(project) => {
        queryClient.invalidateQueries({ queryKey: ['projects'] });
        selectProject(project.id);
        setShowCreate(false);
      }}
    />
  );
}

// ─── Project List View ────────────────────────────────────────────────────────

function ProjectList({
  projects,
  showCreate,
  onCreateOpen,
  onCreateClose,
  onCreated,
}: {
  projects: ProjectSummary[];
  showCreate: boolean;
  onCreateOpen: () => void;
  onCreateClose: () => void;
  onCreated: (p: Project) => void;
}) {
  const { selectProject } = useUiStore();

  return (
    <div className="flex flex-col h-full">
      <header className="flex items-center justify-between px-6 py-4 border-b border-edge">
        <h1 className="text-lg font-semibold text-white">Projects</h1>
        <button
          onClick={onCreateOpen}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          New Project
        </button>
      </header>

      {showCreate && (
        <CreateProjectForm
          onCreated={onCreated}
          onCancel={onCreateClose}
        />
      )}

      <div className="flex-1 overflow-y-auto p-6">
        {projects.length === 0 && !showCreate ? (
          <div className="flex flex-col items-center justify-center h-full gap-4 text-muted">
            <FolderOpen size={48} className="opacity-20" />
            <p className="text-base font-medium">No projects yet</p>
            <p className="text-sm text-center max-w-sm">
              Projects give agents a shared workspace and help you organize tasks, chats, and run
              history around a common goal.
            </p>
            <button
              onClick={onCreateOpen}
              className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
            >
              Create your first project
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 max-w-2xl">
            {projects.map((project) => (
              <button
                key={project.id}
                onClick={() => selectProject(project.id)}
                className="flex items-center gap-4 p-4 rounded-xl border border-edge bg-surface hover:border-accent hover:bg-accent/5 transition-colors text-left group"
              >
                <div className="w-10 h-10 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
                  <FolderOpen size={18} className="text-accent-hover" />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-semibold text-white group-hover:text-accent-hover transition-colors">
                    {project.name}
                  </p>
                  {project.description && (
                    <p className="text-xs text-muted mt-0.5 truncate">{project.description}</p>
                  )}
                  <p className="text-xs text-muted mt-1">
                    Created {new Date(project.createdAt).toLocaleDateString()}
                  </p>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Project Detail View ──────────────────────────────────────────────────────

function ProjectDetail({
  project,
  onDeleted,
}: {
  project: Project;
  onDeleted: () => void;
}) {
  const { projectTab, setProjectTab } = useUiStore();
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(project.name);
  const [editDesc, setEditDesc] = useState(project.description ?? '');

  const { data: workspacePath } = useQuery({
    queryKey: ['project-workspace-path', project.id],
    queryFn: () => projectsApi.getWorkspacePath(project.id),
    staleTime: 60_000,
  });

  async function handleSave() {
    await projectsApi.update(project.id, {
      name: editName.trim() || project.name,
      description: editDesc.trim() || undefined,
    });
    queryClient.invalidateQueries({ queryKey: ['projects'] });
    setEditing(false);
  }

  async function handleDelete() {
    if (!(await confirm(`Delete project "${project.name}"? This cannot be undone.`))) return;
    await projectsApi.delete(project.id);
    onDeleted();
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <header className="flex items-center gap-3 px-6 py-4 border-b border-edge">
        <div className="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
          <FolderOpen size={15} className="text-accent-hover" />
        </div>

        {editing ? (
          <div className="flex-1 flex items-center gap-3">
            <Input
              autoFocus
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') handleSave(); if (e.key === 'Escape') setEditing(false); }}
              className="flex-1 px-3 py-1.5"
            />
            <Input
              value={editDesc}
              onChange={(e) => setEditDesc(e.target.value)}
              placeholder="Description (optional)"
              className="flex-1 px-3 py-1.5 placeholder-muted"
            />
            <button onClick={handleSave} className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors">Save</button>
            <button onClick={() => setEditing(false)} className="text-xs text-muted hover:text-white transition-colors">Cancel</button>
          </div>
        ) : (
          <div className="flex-1 min-w-0">
            <h1 className="text-base font-semibold text-white truncate">{project.name}</h1>
            {project.description && (
              <p className="text-xs text-muted truncate">{project.description}</p>
            )}
          </div>
        )}

        {!editing && workspacePath && (
          <WorkspacePathChip path={workspacePath} className="max-w-[40%]" />
        )}

        {!editing && (
          <div className="flex items-center gap-1">
            <button
              onClick={() => setEditing(true)}
              className="p-1.5 rounded-md text-muted hover:text-white hover:bg-surface transition-colors"
              title="Edit project"
            >
              <Pencil size={14} />
            </button>
            <button
              onClick={handleDelete}
              className="p-1.5 rounded-md text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
              title="Delete project"
            >
              <Trash2 size={14} />
            </button>
          </div>
        )}
      </header>

      {/* Tabs */}
      <div className="flex border-b border-edge px-4">
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => setProjectTab(id)}
            className={cn(
              'flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors',
              projectTab === id
                ? 'border-accent text-accent-hover'
                : 'border-transparent text-muted hover:text-white'
            )}
          >
            <Icon size={14} />
            {label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {projectTab === 'workspace' && <ProjectWorkspaceTab projectId={project.id} />}
        {projectTab === 'agents' && <ProjectAgentsTab projectId={project.id} />}
        {projectTab === 'chat' && <ProjectChatTab projectId={project.id} />}
        {projectTab === 'board' && <ProjectBoardTab projectId={project.id} />}
        {projectTab === 'scheduled' && <ProjectScheduledTab projectId={project.id} />}
        {projectTab === 'workflows' && <ProjectWorkflowsTab projectId={project.id} />}
        {projectTab === 'history' && <ProjectHistoryTab projectId={project.id} />}
      </div>
    </div>
  );
}

// ─── Create Project Form ──────────────────────────────────────────────────────

function CreateProjectForm({
  onCreated,
  onCancel,
}: {
  onCreated: (p: Project) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [boardPresetId, setBoardPresetId] = useState<'starter' | 'lean'>('starter');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleCreate() {
    if (!name.trim()) return;
    setCreating(true);
    setError(null);
    try {
      const project = await projectsApi.create({
        name: name.trim(),
        description: description.trim() || undefined,
        boardPresetId,
      });
      onCreated(project);
    } catch (e) {
      setError(String(e));
      setCreating(false);
    }
  }

  return (
    <div className="mx-6 my-4 p-4 rounded-xl border border-edge bg-surface space-y-3">
      <h2 className="text-sm font-semibold text-white">New Project</h2>
      <div className="space-y-2">
        <Input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') handleCreate(); if (e.key === 'Escape') onCancel(); }}
          placeholder="Project name"
          className="bg-background px-3 py-2 placeholder-muted"
        />
        <Input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Description (optional)"
          className="bg-background px-3 py-2 placeholder-muted"
        />
        <SimpleSelect
          value={boardPresetId}
          onValueChange={(v) => setBoardPresetId(v as 'starter' | 'lean')}
          className="bg-background px-3 py-2"
          options={[
            { value: 'starter', label: 'Starter board' },
            { value: 'lean', label: 'Lean board' },
          ]}
        />
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}
      <div className="flex gap-2">
        <button
          onClick={handleCreate}
          disabled={!name.trim() || creating}
          className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium transition-colors"
        >
          {creating ? 'Creating…' : 'Create Project'}
        </button>
        <button onClick={onCancel} className="px-4 py-2 rounded-lg border border-edge text-muted hover:text-white hover:border-edge-hover text-sm transition-colors">
          Cancel
        </button>
      </div>
    </div>
  );
}
