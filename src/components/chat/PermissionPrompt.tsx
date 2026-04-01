import { useState } from 'react';
import { Shield, ShieldAlert, Check, X, ChevronRight } from 'lucide-react';
import { permissionsApi } from '../../api/permissions';
import { usePermissionStore } from '../../store/permissionStore';
import { PermissionRule } from '../../types';

interface PermissionPromptProps {
  requestId: string;
  toolName: string;
  toolInput: Record<string, unknown>;
  riskLevel: "moderate" | "dangerous";
  riskDescription: string;
  suggestedPattern: string;
  agentId?: string;
  resolved?: "allow" | "always_allow" | "deny";
}

export function PermissionPrompt({
  requestId,
  toolName,
  toolInput,
  riskLevel,
  riskDescription,
  suggestedPattern,
  agentId: agentIdProp,
  resolved,
}: PermissionPromptProps) {
  // Fall back to getting agentId from the permission store if not provided as prop
  const pendingReq = usePermissionStore((s) => s.pending[requestId]);
  const agentId = agentIdProp || pendingReq?.agentId || "";
  const [expanded, setExpanded] = useState(false);
  const [localResolved, setLocalResolved] = useState<string | undefined>(resolved);
  const resolveRequest = usePermissionStore((s) => s.resolveRequest);

  const isDangerous = riskLevel === "dangerous";
  const borderColor = isDangerous ? "border-red-500/40" : "border-amber-500/40";
  const bgColor = isDangerous ? "bg-red-500/5" : "bg-amber-500/5";
  const iconColor = isDangerous ? "text-red-400" : "text-amber-400";
  const ShieldIcon = isDangerous ? ShieldAlert : Shield;

  const handleRespond = async (response: "allow" | "always_allow" | "deny") => {
    setLocalResolved(response);
    resolveRequest(requestId, response);

    try {
      await permissionsApi.respond(requestId, response);

      if (response === "always_allow") {
        const rule: PermissionRule = {
          id: crypto.randomUUID(),
          tool: toolName,
          pattern: suggestedPattern,
          decision: "allow",
          createdAt: new Date().toISOString(),
          description: `Auto-created: always allow ${toolName} matching "${suggestedPattern}"`,
        };
        await permissionsApi.saveRule(agentId, rule);
      }
    } catch {
      // If the backend already resolved (e.g. timeout), silently ignore
    }
  };

  const inputStr = JSON.stringify(toolInput, null, 2);

  // Resolved state — compact display
  if (localResolved) {
    const isAllowed = localResolved === "allow" || localResolved === "always_allow";
    return (
      <div className={`rounded-lg border ${isAllowed ? 'border-emerald-500/30 bg-emerald-500/5' : 'border-red-500/30 bg-red-500/5'} px-3 py-2 flex items-center gap-2`}>
        {isAllowed ? (
          <Check size={12} className="text-emerald-400 shrink-0" />
        ) : (
          <X size={12} className="text-red-400 shrink-0" />
        )}
        <span className="text-xs text-muted">
          {localResolved === "allow" && "Allowed"}
          {localResolved === "always_allow" && `Always allowed (pattern: "${suggestedPattern}")`}
          {localResolved === "deny" && "Denied"}
        </span>
        <span className="text-xs font-medium text-secondary ml-1">{toolName}</span>
      </div>
    );
  }

  // Pending state — full prompt
  return (
    <div className={`rounded-lg border ${borderColor} ${bgColor} overflow-hidden`}>
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2">
        <ShieldIcon size={14} className={iconColor} />
        <span className="text-xs font-medium text-white">Permission Required</span>
        <span className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${isDangerous ? 'bg-red-500/20 text-red-400' : 'bg-amber-500/20 text-amber-400'}`}>
          {isDangerous ? "Dangerous" : "Moderate"}
        </span>
      </div>

      {/* Description */}
      <div className="px-3 pb-2">
        <p className="text-xs text-secondary">{riskDescription}</p>
        <p className="text-xs text-muted mt-1">
          Tool: <span className="font-mono text-warning">{toolName}</span>
        </p>
      </div>

      {/* Expandable input details */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 px-3 py-1 text-[10px] text-muted hover:text-secondary transition-colors w-full text-left"
      >
        <ChevronRight
          size={10}
          className={`transition-transform ${expanded ? 'rotate-90' : ''}`}
        />
        View details
      </button>
      {expanded && (
        <pre className="px-3 pb-2 text-xs font-mono text-secondary whitespace-pre-wrap break-all overflow-x-auto max-h-40 overflow-y-auto">
          {inputStr}
        </pre>
      )}

      {/* Action buttons */}
      <div className="flex items-center gap-2 px-3 py-2 border-t border-white/5">
        <button
          onClick={() => handleRespond("allow")}
          className="px-3 py-1.5 text-xs font-medium rounded bg-emerald-600 hover:bg-emerald-500 text-white transition-colors"
        >
          Allow
        </button>
        <button
          onClick={() => handleRespond("always_allow")}
          className="px-3 py-1.5 text-xs font-medium rounded border border-emerald-600/50 text-emerald-400 hover:bg-emerald-600/10 transition-colors"
          title={`Will always allow: ${toolName} matching "${suggestedPattern}"`}
        >
          Always Allow
        </button>
        <button
          onClick={() => handleRespond("deny")}
          className="px-3 py-1.5 text-xs font-medium rounded border border-red-500/50 text-red-400 hover:bg-red-500/10 transition-colors"
        >
          Deny
        </button>
        <span className="text-[10px] text-muted ml-auto">
          Always: <span className="font-mono">{suggestedPattern}</span>
        </span>
      </div>
    </div>
  );
}
