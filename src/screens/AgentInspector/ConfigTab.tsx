import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Save, Key, Trash2, Check, ChevronDown } from "lucide-react";
import * as Select from "@radix-ui/react-select";
import * as Checkbox from "@radix-ui/react-checkbox";
import * as Slider from "@radix-ui/react-slider";
import { workspaceApi } from "../../api/workspace";
import { llmApi } from "../../api/llm";
import { AgentWorkspaceConfig } from "../../types";
import { confirm } from "@tauri-apps/plugin-dialog";

const AVAILABLE_TOOLS = [
  { id: "shell_command", label: "Shell Commands" },
  { id: "read_file", label: "Read Files" },
  { id: "write_file", label: "Write Files" },
  { id: "list_files", label: "List Files" },
  { id: "finish", label: "Finish (always enabled)" },
];

const MODEL_OPTIONS: Record<string, { label: string; value: string }[]> = {
  anthropic: [
    { label: "Claude Sonnet 4", value: "claude-sonnet-4-20250514" },
    { label: "Claude Haiku 3.5", value: "claude-haiku-4-5-20251001" },
  ],
  minimax: [
    { label: "MiniMax M2.7", value: "MiniMax-M2.7" },
    { label: "MiniMax M2.7 Highspeed", value: "MiniMax-M2.7-highspeed" },
    { label: "MiniMax M2.5", value: "MiniMax-M2.5" },
    { label: "MiniMax M2.5 Highspeed", value: "MiniMax-M2.5-highspeed" },
    { label: "MiniMax M2.1", value: "MiniMax-M2.1" },
    { label: "MiniMax M2.1 Highspeed", value: "MiniMax-M2.1-highspeed" },
    { label: "MiniMax M2", value: "MiniMax-M2" },
  ],
};

export function ConfigTab({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [config, setConfig] = useState<AgentWorkspaceConfig | null>(null);

  // API key state
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [showKeyInput, setShowKeyInput] = useState(false);

  const { data: loadedConfig } = useQuery({
    queryKey: ["agent-config", agentId],
    queryFn: () => workspaceApi.getConfig(agentId),
  });

  useEffect(() => {
    if (loadedConfig) {
      setConfig(loadedConfig);
      // Check API key status for the provider
      llmApi.hasApiKey(loadedConfig.provider).then(setHasKey).catch(() => setHasKey(false));
    }
  }, [loadedConfig]);

  async function handleSave() {
    if (!config) return;
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    try {
      await workspaceApi.updateConfig(agentId, config);
      queryClient.invalidateQueries({ queryKey: ["agent-config", agentId] });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      setSaveError(String(err));
    }
    setSaving(false);
  }

  async function handleSetApiKey() {
    if (!config || !keyInput.trim()) return;
    try {
      await llmApi.setApiKey(config.provider, keyInput.trim());
      setHasKey(true);
      setKeyInput("");
      setShowKeyInput(false);
    } catch (err) {
      console.error("Failed to set API key:", err);
    }
  }

  async function handleDeleteApiKey() {
    if (!config) return;
    if (!await confirm("Remove API key?")) return;
    try {
      await llmApi.deleteApiKey(config.provider);
      setHasKey(false);
    } catch (err) {
      console.error("Failed to delete API key:", err);
    }
  }

  function toggleTool(toolId: string) {
    if (!config) return;
    const tools = config.allowedTools.includes(toolId)
      ? config.allowedTools.filter((t) => t !== toolId)
      : [...config.allowedTools, toolId];
    setConfig({ ...config, allowedTools: tools });
  }

  if (!config) {
    return <div className="p-6 text-[#64748b] text-sm">Loading configuration...</div>;
  }

  const models = MODEL_OPTIONS[config.provider] ?? [];

  return (
    <div className="p-6 space-y-6 max-w-2xl h-full overflow-y-auto">
      

      {/* Provider & Model */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Model</h4>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="text-xs text-[#64748b] mb-1 block">Provider</label>
            <Select.Root
              value={config.provider}
              onValueChange={(value) => {
                setConfig({ ...config, provider: value });
                llmApi.hasApiKey(value).then(setHasKey).catch(() => setHasKey(false));
              }}
            >
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]">
                <Select.Value />
                <Select.Icon><ChevronDown size={14} className="text-[#64748b]" /></Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-[#1a1d27] border border-[#2a2d3e] shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    <Select.Item value="anthropic" className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20">
                      <Select.ItemText>Anthropic</Select.ItemText>
                    </Select.Item>
                    <Select.Item value="minimax" className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20">
                      <Select.ItemText>MiniMax</Select.ItemText>
                    </Select.Item>
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
          <div>
            <label className="text-xs text-[#64748b] mb-1 block">Model</label>
            <Select.Root
              value={config.model}
              onValueChange={(value) => setConfig({ ...config, model: value })}
            >
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]">
                <Select.Value />
                <Select.Icon><ChevronDown size={14} className="text-[#64748b]" /></Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-[#1a1d27] border border-[#2a2d3e] shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {models.map((m) => (
                      <Select.Item key={m.value} value={m.value} className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20">
                        <Select.ItemText>{m.label}</Select.ItemText>
                      </Select.Item>
                    ))}
                    {!models.find((m) => m.value === config.model) && (
                      <Select.Item value={config.model} className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20">
                        <Select.ItemText>{config.model}</Select.ItemText>
                      </Select.Item>
                    )}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
        </div>
      </section>

      {/* API Key */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">API Key</h4>
        <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-4">
          {hasKey ? (
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Check size={14} className="text-emerald-400" />
                <span className="text-sm text-emerald-400">
                  {config.provider} API key configured
                </span>
              </div>
              <button
                onClick={handleDeleteApiKey}
                className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10"
              >
                <Trash2 size={11} /> Remove
              </button>
            </div>
          ) : showKeyInput ? (
            <div className="space-y-2">
              <input
                type="password"
                placeholder={`Enter ${config.provider} API key...`}
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleSetApiKey()}
                autoFocus
                className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm font-mono focus:outline-none focus:border-[#6366f1]"
              />
              <div className="flex gap-2">
                <button
                  onClick={handleSetApiKey}
                  disabled={!keyInput.trim()}
                  className="px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-xs font-medium"
                >
                  Save Key
                </button>
                <button
                  onClick={() => setShowKeyInput(false)}
                  className="px-3 py-1.5 rounded-lg text-[#64748b] hover:text-white text-xs"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowKeyInput(true)}
              className="flex items-center gap-2 px-3 py-2 rounded-lg border border-dashed border-[#4a4d6e] text-[#94a3b8] hover:text-white hover:border-[#6366f1] text-sm w-full transition-colors"
            >
              <Key size={14} />
              Set {config.provider} API key
            </button>
          )}
        </div>
      </section>

      {/* Temperature */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Temperature</h4>
        <div className="flex items-center gap-4">
          <Slider.Root
            min={0}
            max={1}
            step={0.05}
            value={[config.temperature]}
            onValueChange={([v]) => setConfig({ ...config, temperature: v })}
            className="relative flex items-center flex-1 h-5 select-none touch-none"
          >
            <Slider.Track className="relative grow h-1 rounded-full bg-[#2a2d3e]">
              <Slider.Range className="absolute h-full rounded-full bg-[#6366f1]" />
            </Slider.Track>
            <Slider.Thumb className="block w-4 h-4 rounded-full bg-white shadow-md border-2 border-[#6366f1] focus:outline-none focus:ring-2 focus:ring-[#6366f1]/40" />
          </Slider.Root>
          <span className="text-sm text-white w-10 text-right font-mono">
            {config.temperature.toFixed(2)}
          </span>
        </div>
      </section>

      {/* Limits */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Limits</h4>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="text-xs text-[#64748b] mb-1 block">Max Iterations</label>
            <input
              type="number"
              min={1}
              max={100}
              value={config.maxIterations}
              onChange={(e) => setConfig({ ...config, maxIterations: parseInt(e.target.value) || 25 })}
              className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
            />
          </div>
          <div>
            <label className="text-xs text-[#64748b] mb-1 block">Max Total Tokens</label>
            <input
              type="number"
              min={1000}
              step={10000}
              value={config.maxTotalTokens}
              onChange={(e) => setConfig({ ...config, maxTotalTokens: parseInt(e.target.value) || 200000 })}
              className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
            />
          </div>
        </div>
      </section>

      {/* Tools */}
      <section className="space-y-3">
        <h4 className="text-sm font-semibold text-white">Allowed Tools</h4>
        <div className="space-y-2">
          {AVAILABLE_TOOLS.map((tool) => {
            const isFinish = tool.id === "finish";
            const checked = isFinish || config.allowedTools.includes(tool.id);
            return (
              <div
                key={tool.id}
                onClick={() => !isFinish && toggleTool(tool.id)}
                className={`flex items-center gap-3 px-3 py-2 rounded-lg border transition-colors cursor-pointer ${
                  checked
                    ? "border-[#6366f1]/30 bg-[#6366f1]/5"
                    : "border-[#2a2d3e] bg-[#1a1d27]"
                } ${isFinish ? "opacity-60 cursor-not-allowed" : ""}`}
              >
                <Checkbox.Root
                  checked={checked}
                  disabled={isFinish}
                  onCheckedChange={() => !isFinish && toggleTool(tool.id)}
                  className="flex items-center justify-center w-4 h-4 rounded border border-[#4a4d6e] bg-[#0f1117] data-[state=checked]:bg-[#6366f1] data-[state=checked]:border-[#6366f1]"
                >
                  <Checkbox.Indicator>
                    <Check size={10} className="text-white" />
                  </Checkbox.Indicator>
                </Checkbox.Root>
                <span className="text-sm text-white">{tool.label}</span>
                <span className="text-xs text-[#64748b] font-mono">{tool.id}</span>
              </div>
            );
          })}
        </div>
      </section>

      {/* Save */}
      <div className="space-y-2 pb-4">
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-sm font-medium transition-colors"
        >
          {saved ? <Check size={14} /> : <Save size={14} />}
          {saving ? "Saving..." : saved ? "Saved" : "Save Configuration"}
        </button>
        {saveError && (
          <div className="px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-xs">
            {saveError}
          </div>
        )}
      </div>
    </div>
  );
}
