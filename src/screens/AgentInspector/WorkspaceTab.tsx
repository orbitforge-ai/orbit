import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  File,
  Folder,
  ChevronRight,
  Save,
  Plus,
  Trash2,
  ArrowLeft,
} from "lucide-react";
import { workspaceApi } from "../../api/workspace";
import { FileEntry } from "../../types";
import { confirm } from "@tauri-apps/plugin-dialog";

export function WorkspaceTab({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const [currentPath, setCurrentPath] = useState(".");
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState("");
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newFileName, setNewFileName] = useState("");

  const { data: files = [], isLoading } = useQuery({
    queryKey: ["workspace-files", agentId, currentPath],
    queryFn: () => workspaceApi.listFiles(agentId, currentPath),
    refetchInterval: 10_000,
  });

  async function handleOpenFile(file: FileEntry) {
    if (file.isDir) {
      const newPath = currentPath === "." ? file.name : `${currentPath}/${file.name}`;
      setCurrentPath(newPath);
      setEditingFile(null);
      return;
    }
    const path = currentPath === "." ? file.name : `${currentPath}/${file.name}`;
    try {
      const content = await workspaceApi.readFile(agentId, path);
      setEditingFile(path);
      setFileContent(content);
    } catch (err) {
      console.error("Failed to read file:", err);
    }
  }

  async function handleSave() {
    if (!editingFile) return;
    setSaving(true);
    try {
      await workspaceApi.writeFile(agentId, editingFile, fileContent);
      queryClient.invalidateQueries({ queryKey: ["workspace-files", agentId] });
    } catch (err) {
      console.error("Failed to save file:", err);
    }
    setSaving(false);
  }

  async function handleDelete(file: FileEntry) {
    const path = currentPath === "." ? file.name : `${currentPath}/${file.name}`;
    if (!await confirm(`Delete "${file.name}"?`)) return;
    try {
      await workspaceApi.deleteFile(agentId, path);
      if (editingFile === path) {
        setEditingFile(null);
      }
      queryClient.invalidateQueries({ queryKey: ["workspace-files", agentId] });
    } catch (err) {
      console.error("Failed to delete:", err);
    }
  }

  async function handleCreate() {
    if (!newFileName.trim()) return;
    const path =
      currentPath === "."
        ? newFileName.trim()
        : `${currentPath}/${newFileName.trim()}`;
    try {
      await workspaceApi.writeFile(agentId, path, "");
      setCreating(false);
      setNewFileName("");
      queryClient.invalidateQueries({ queryKey: ["workspace-files", agentId] });
      // Open the new file for editing
      setEditingFile(path);
      setFileContent("");
    } catch (err) {
      console.error("Failed to create file:", err);
    }
  }

  function navigateUp() {
    if (currentPath === ".") return;
    const parts = currentPath.split("/");
    parts.pop();
    setCurrentPath(parts.length === 0 ? "." : parts.join("/"));
    setEditingFile(null);
  }

  const isSpecialFile = (name: string) =>
    name === "system_prompt.md" || name === "config.json";

  return (
    <div className="flex h-full">
      {/* File tree */}
      <div className="w-[260px] flex flex-col border-r border-[#2a2d3e]">
        <div className="flex items-center justify-between px-4 py-3 border-b border-[#2a2d3e]">
          <div className="flex items-center gap-2">
            {currentPath !== "." && (
              <button
                onClick={navigateUp}
                className="p-1 rounded text-[#64748b] hover:text-white hover:bg-[#2a2d3e]"
              >
                <ArrowLeft size={14} />
              </button>
            )}
            <span className="text-xs text-[#64748b] font-mono truncate">
              {currentPath === "." ? "/" : `/${currentPath}`}
            </span>
          </div>
          <button
            onClick={() => setCreating(true)}
            className="p-1 rounded text-[#64748b] hover:text-[#818cf8] hover:bg-[#6366f1]/10"
          >
            <Plus size={14} />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
          {isLoading && (
            <div className="text-center py-4 text-[#64748b] text-xs">Loading...</div>
          )}

          {creating && (
            <div className="flex items-center gap-1 px-2 py-1.5">
              <input
                type="text"
                placeholder="filename.md"
                value={newFileName}
                onChange={(e) => setNewFileName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleCreate();
                  if (e.key === "Escape") setCreating(false);
                }}
                autoFocus
                className="flex-1 px-2 py-1 rounded bg-[#0f1117] border border-[#6366f1] text-white text-xs focus:outline-none"
              />
            </div>
          )}

          {files.map((file) => {
            const filePath =
              currentPath === "." ? file.name : `${currentPath}/${file.name}`;
            const isActive = editingFile === filePath;

            return (
              <div
                key={file.name}
                className={`flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer group ${
                  isActive
                    ? "bg-[#6366f1]/15 text-white"
                    : "text-[#94a3b8] hover:bg-[#1a1d27] hover:text-white"
                }`}
                onClick={() => handleOpenFile(file)}
              >
                {file.isDir ? (
                  <Folder size={14} className="text-[#818cf8] flex-shrink-0" />
                ) : (
                  <File
                    size={14}
                    className={`flex-shrink-0 ${
                      isSpecialFile(file.name) ? "text-amber-400" : "text-[#64748b]"
                    }`}
                  />
                )}
                <span className="text-xs truncate flex-1 font-mono">{file.name}</span>
                {file.isDir && (
                  <ChevronRight size={12} className="text-[#64748b] flex-shrink-0" />
                )}
                {!file.isDir && !isSpecialFile(file.name) && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDelete(file);
                    }}
                    className="hidden group-hover:block p-0.5 rounded text-[#64748b] hover:text-red-400"
                  >
                    <Trash2 size={11} />
                  </button>
                )}
              </div>
            );
          })}

          {!isLoading && files.length === 0 && (
            <div className="text-center py-4 text-[#64748b] text-xs">
              Empty directory
            </div>
          )}
        </div>
      </div>

      {/* Editor */}
      <div className="flex-1 flex flex-col">
        {editingFile ? (
          <>
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-[#2a2d3e]">
              <span className="text-xs text-[#94a3b8] font-mono">{editingFile}</span>
              <button
                onClick={handleSave}
                disabled={saving}
                className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-xs font-medium"
              >
                <Save size={11} />
                {saving ? "Saving..." : "Save"}
              </button>
            </div>
            <textarea
              value={fileContent}
              onChange={(e) => setFileContent(e.target.value)}
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === "s") {
                  e.preventDefault();
                  handleSave();
                }
              }}
              spellCheck={false}
              className="flex-1 p-4 bg-[#0f1117] text-[#e2e8f0] text-sm font-mono resize-none focus:outline-none leading-relaxed"
            />
          </>
        ) : (
          <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
            Select a file to edit
          </div>
        )}
      </div>
    </div>
  );
}
