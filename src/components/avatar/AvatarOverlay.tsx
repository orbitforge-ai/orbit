import { Eye, EyeOff, Volume2, VolumeX } from 'lucide-react';
import { ResolvedArchetype, AvatarState } from './types';
import { AVATAR_SVG_MAP } from './avatarSvgs';
import { ToolBadge } from './ToolBadge';

interface AvatarOverlayProps {
  archetype: ResolvedArchetype;
  state: AvatarState;
  visible: boolean;
  speakAloud: boolean;
  onToggleVisible: () => void;
  onToggleSpeakAloud: () => void;
}

function SpeechBubble({ text }: { text: string }) {
  const display = text.trim();
  if (!display) return null;

  return (
    <div
      className="absolute bottom-full mb-3 right-0 w-48 rounded-xl bg-surface border border-edge px-3 py-2 text-xs text-secondary leading-snug shadow-xl"
      style={{ animation: 'bubble-appear 0.2s ease-out forwards' }}
    >
      <span className="line-clamp-3">{display}</span>
      {/* Triangle pointer */}
      <div
        className="absolute -bottom-[6px] right-6 w-3 h-3 bg-surface border-r border-b border-edge"
        style={{ transform: 'rotate(45deg)' }}
      />
    </div>
  );
}

function animationClass(state: AvatarState): string {
  switch (state.phase) {
    case 'thinking':   return 'avatar-thinking';
    case 'speaking':   return 'avatar-speaking';
    case 'using-tool': return 'avatar-idle';
    default:           return 'avatar-idle';
  }
}

export function AvatarOverlay({
  archetype,
  state,
  visible,
  speakAloud,
  onToggleVisible,
  onToggleSpeakAloud,
}: AvatarOverlayProps) {
  const AvatarSvg = AVATAR_SVG_MAP[archetype];

  return (
    <>
      {/* Toggle controls — always visible in top-right of message area */}
      <div className="absolute top-3 right-3 z-20 flex items-center gap-1">
        <button
          onClick={onToggleSpeakAloud}
          title={speakAloud ? 'Mute voice' : 'Enable voice'}
          className="p-1.5 rounded-md text-muted hover:text-white hover:bg-surface border border-transparent hover:border-edge transition-colors"
        >
          {speakAloud ? <Volume2 size={13} /> : <VolumeX size={13} />}
        </button>
        <button
          onClick={onToggleVisible}
          title={visible ? 'Hide avatar' : 'Show avatar'}
          className="p-1.5 rounded-md text-muted hover:text-white hover:bg-surface border border-transparent hover:border-edge transition-colors"
        >
          {visible ? <Eye size={13} /> : <EyeOff size={13} />}
        </button>
      </div>

      {/* Avatar body */}
      {visible && (
        <div className="absolute bottom-4 right-4 z-10 pointer-events-none select-none">
          {/* Speech bubble */}
          <div className="relative">
            {state.phase === 'speaking' && <SpeechBubble text={state.text} />}

            {/* Avatar character */}
            <div
              className={`relative w-20 h-20 avatar-entry ${animationClass(state)}`}
              style={{ willChange: 'transform' }}
            >
              <AvatarSvg size={80} />
              {state.phase === 'using-tool' && <ToolBadge toolName={state.toolName} />}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
