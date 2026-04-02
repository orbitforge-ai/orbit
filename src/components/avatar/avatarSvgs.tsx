import { ResolvedArchetype, AvatarSvgProps } from './types';

// ─── Fox ─────────────────────────────────────────────────────────────────────
// Clever, pointed ears, narrow muzzle — high directness + humor

export function FoxAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Ears */}
      <polygon points="16,38 24,14 34,36" fill="#f97316" />
      <polygon points="46,36 56,14 64,38" fill="#f97316" />
      <polygon points="20,36 24,20 32,35" fill="#fcd34d" />
      <polygon points="48,35 56,20 60,36" fill="#fcd34d" />
      {/* Head */}
      <ellipse cx="40" cy="46" rx="22" ry="20" fill="#f97316" />
      {/* Muzzle */}
      <ellipse cx="40" cy="54" rx="11" ry="8" fill="#fcd34d" />
      {/* Eyes */}
      <ellipse cx="32" cy="44" rx="4" ry="4.5" fill="white" className="avatar-blink" style={{ transformOrigin: '32px 44px' }} />
      <ellipse cx="48" cy="44" rx="4" ry="4.5" fill="white" className="avatar-blink" style={{ transformOrigin: '48px 44px' }} />
      <circle cx="33" cy="44" r="2.5" fill="#1e1b4b" />
      <circle cx="49" cy="44" r="2.5" fill="#1e1b4b" />
      <circle cx="34" cy="43" r="0.8" fill="white" />
      <circle cx="50" cy="43" r="0.8" fill="white" />
      {/* Nose */}
      <ellipse cx="40" cy="51" rx="2.5" ry="1.8" fill="#7c2d12" />
    </svg>
  );
}

// ─── Bear ─────────────────────────────────────────────────────────────────────
// Warm, round ears, large round face — high warmth

export function BearAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Ears */}
      <circle cx="21" cy="25" r="10" fill="#92400e" />
      <circle cx="59" cy="25" r="10" fill="#92400e" />
      <circle cx="21" cy="25" r="6" fill="#b45309" />
      <circle cx="59" cy="25" r="6" fill="#b45309" />
      {/* Head */}
      <ellipse cx="40" cy="47" rx="24" ry="22" fill="#b45309" />
      {/* Muzzle */}
      <ellipse cx="40" cy="55" rx="12" ry="9" fill="#d97706" />
      {/* Eyes */}
      <ellipse cx="32" cy="44" rx="4" ry="4.5" fill="white" className="avatar-blink" style={{ transformOrigin: '32px 44px' }} />
      <ellipse cx="48" cy="44" rx="4" ry="4.5" fill="white" className="avatar-blink" style={{ transformOrigin: '48px 44px' }} />
      <circle cx="33" cy="44" r="2.8" fill="#1e1b4b" />
      <circle cx="49" cy="44" r="2.8" fill="#1e1b4b" />
      <circle cx="34" cy="43" r="1" fill="white" />
      <circle cx="50" cy="43" r="1" fill="white" />
      {/* Nose */}
      <ellipse cx="40" cy="52" rx="3" ry="2" fill="#1c1917" />
    </svg>
  );
}

// ─── Owl ──────────────────────────────────────────────────────────────────────
// Calm, analytical — large eyes dominate, feather tufts

export function OwlAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Feather tufts */}
      <polygon points="28,22 32,10 36,22" fill="#78716c" />
      <polygon points="44,22 48,10 52,22" fill="#78716c" />
      {/* Head */}
      <ellipse cx="40" cy="46" rx="22" ry="22" fill="#a8a29e" />
      {/* Facial disc */}
      <ellipse cx="40" cy="48" rx="17" ry="17" fill="#d6d3d1" />
      {/* Eye rings */}
      <circle cx="31" cy="45" r="9" fill="#78716c" />
      <circle cx="49" cy="45" r="9" fill="#78716c" />
      {/* Eyes — large, prominent */}
      <circle cx="31" cy="45" r="7" fill="#fbbf24" className="avatar-blink" style={{ transformOrigin: '31px 45px' }} />
      <circle cx="49" cy="45" r="7" fill="#fbbf24" className="avatar-blink" style={{ transformOrigin: '49px 45px' }} />
      <circle cx="31" cy="45" r="4" fill="#1e1b4b" />
      <circle cx="49" cy="45" r="4" fill="#1e1b4b" />
      <circle cx="32.5" cy="43.5" r="1.2" fill="white" />
      <circle cx="50.5" cy="43.5" r="1.2" fill="white" />
      {/* Beak */}
      <polygon points="37,52 43,52 40,57" fill="#f59e0b" />
    </svg>
  );
}

// ─── Spark ────────────────────────────────────────────────────────────────────
// High humor + warmth — starburst body, expressive

export function SparkAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Radiating spikes */}
      {[0,45,90,135,180,225,270,315].map((deg, i) => {
        const rad = (deg * Math.PI) / 180;
        const x1 = 40 + 18 * Math.cos(rad);
        const y1 = 42 + 18 * Math.sin(rad);
        const x2 = 40 + 30 * Math.cos(rad);
        const y2 = 42 + 30 * Math.sin(rad);
        return (
          <line key={i} x1={x1} y1={y1} x2={x2} y2={y2}
            stroke="#fbbf24" strokeWidth="3" strokeLinecap="round" />
        );
      })}
      {/* Core body */}
      <circle cx="40" cy="42" r="18" fill="#fde68a" />
      <circle cx="40" cy="42" r="14" fill="#fbbf24" />
      {/* Eyes */}
      <ellipse cx="34" cy="40" rx="3.5" ry="4" fill="white" className="avatar-blink" style={{ transformOrigin: '34px 40px' }} />
      <ellipse cx="46" cy="40" rx="3.5" ry="4" fill="white" className="avatar-blink" style={{ transformOrigin: '46px 40px' }} />
      <circle cx="35" cy="40" r="2.2" fill="#1e1b4b" />
      <circle cx="47" cy="40" r="2.2" fill="#1e1b4b" />
      <circle cx="35.8" cy="39" r="0.7" fill="white" />
      <circle cx="47.8" cy="39" r="0.7" fill="white" />
      {/* Smile */}
      <path d="M34 46 Q40 51 46 46" stroke="#92400e" strokeWidth="1.5" strokeLinecap="round" fill="none" />
    </svg>
  );
}

// ─── Cat ──────────────────────────────────────────────────────────────────────
// Cool, high directness + low warmth — almond eyes, whiskers

export function CatAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Ears */}
      <polygon points="18,36 22,14 34,34" fill="#6366f1" />
      <polygon points="46,34 58,14 62,36" fill="#6366f1" />
      <polygon points="22,34 25,20 33,33" fill="#818cf8" />
      <polygon points="47,33 55,20 58,34" fill="#818cf8" />
      {/* Head */}
      <ellipse cx="40" cy="46" rx="22" ry="20" fill="#6366f1" />
      {/* Muzzle */}
      <ellipse cx="40" cy="55" rx="10" ry="7" fill="#818cf8" />
      {/* Almond eyes */}
      <ellipse cx="32" cy="44" rx="5" ry="4" fill="#a5f3fc" className="avatar-blink" style={{ transformOrigin: '32px 44px' }} />
      <ellipse cx="48" cy="44" rx="5" ry="4" fill="#a5f3fc" className="avatar-blink" style={{ transformOrigin: '48px 44px' }} />
      <ellipse cx="32" cy="44" rx="2.5" ry="3.5" fill="#1e1b4b" />
      <ellipse cx="48" cy="44" rx="2.5" ry="3.5" fill="#1e1b4b" />
      <circle cx="32.5" cy="43" r="0.7" fill="white" />
      <circle cx="48.5" cy="43" r="0.7" fill="white" />
      {/* Nose */}
      <polygon points="38.5,51 41.5,51 40,53" fill="#c7d2fe" />
      {/* Whiskers */}
      <line x1="18" y1="51" x2="34" y2="52" stroke="#a5b4fc" strokeWidth="1" strokeOpacity="0.7" />
      <line x1="18" y1="54" x2="34" y2="54" stroke="#a5b4fc" strokeWidth="1" strokeOpacity="0.7" />
      <line x1="46" y1="52" x2="62" y2="51" stroke="#a5b4fc" strokeWidth="1" strokeOpacity="0.7" />
      <line x1="46" y1="54" x2="62" y2="54" stroke="#a5b4fc" strokeWidth="1" strokeOpacity="0.7" />
    </svg>
  );
}

// ─── Bot ──────────────────────────────────────────────────────────────────────
// Neutral, balanced — square head, visor eye, antenna

export function BotAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Antenna */}
      <line x1="40" y1="14" x2="40" y2="24" stroke="#818cf8" strokeWidth="2.5" strokeLinecap="round" />
      <circle cx="40" cy="12" r="3" fill="#6366f1" />
      {/* Head — rounded square */}
      <rect x="16" y="24" width="48" height="42" rx="10" ry="10" fill="#3730a3" />
      <rect x="18" y="26" width="44" height="38" rx="8" ry="8" fill="#4338ca" />
      {/* Visor bar */}
      <rect x="22" y="36" width="36" height="12" rx="3" fill="#0ea5e9" opacity="0.9" />
      <rect x="22" y="36" width="36" height="12" rx="3" fill="url(#visor-shine)" />
      <defs>
        <linearGradient id="visor-shine" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="white" stopOpacity="0.2" />
          <stop offset="100%" stopColor="white" stopOpacity="0" />
        </linearGradient>
      </defs>
      {/* Pupils inside visor */}
      <circle cx="32" cy="42" r="3" fill="#7dd3fc" className="avatar-blink" style={{ transformOrigin: '32px 42px' }} />
      <circle cx="48" cy="42" r="3" fill="#7dd3fc" className="avatar-blink" style={{ transformOrigin: '48px 42px' }} />
      <circle cx="32" cy="42" r="1.5" fill="#1e3a8a" />
      <circle cx="48" cy="42" r="1.5" fill="#1e3a8a" />
      {/* Mouth LED strip */}
      <rect x="28" y="54" width="24" height="4" rx="2" fill="#6366f1" opacity="0.8" />
      {/* Side bolts */}
      <circle cx="19" cy="42" r="3" fill="#3730a3" stroke="#6366f1" strokeWidth="1.5" />
      <circle cx="61" cy="42" r="3" fill="#3730a3" stroke="#6366f1" strokeWidth="1.5" />
    </svg>
  );
}

// ─── Sage ─────────────────────────────────────────────────────────────────────
// Low humor, any traits — hooded, contemplative, beard

export function SageAvatar({ size = 80, className = '' }: AvatarSvgProps) {
  return (
    <svg viewBox="0 0 80 80" width={size} height={size} className={className} fill="none">
      {/* Hood */}
      <ellipse cx="40" cy="36" rx="26" ry="28" fill="#4c1d95" />
      <ellipse cx="40" cy="30" rx="20" ry="22" fill="#7c3aed" />
      {/* Face */}
      <ellipse cx="40" cy="46" rx="16" ry="18" fill="#fde68a" />
      {/* Closed eyes — contemplative */}
      <path d="M29 43 Q32 41 35 43" stroke="#92400e" strokeWidth="1.5" strokeLinecap="round" fill="none" />
      <path d="M45 43 Q48 41 51 43" stroke="#92400e" strokeWidth="1.5" strokeLinecap="round" fill="none" />
      {/* Beard */}
      <ellipse cx="40" cy="60" rx="12" ry="8" fill="#e5e7eb" />
      <ellipse cx="40" cy="56" rx="10" ry="6" fill="#f3f4f6" />
      {/* Hood shadow */}
      <ellipse cx="40" cy="28" rx="14" ry="8" fill="#4c1d95" opacity="0.5" />
      {/* Star on hood */}
      <circle cx="40" cy="20" r="3" fill="#fbbf24" opacity="0.8" />
      <circle cx="40" cy="20" r="1.5" fill="#f59e0b" />
    </svg>
  );
}

// ─── Map ──────────────────────────────────────────────────────────────────────

export const AVATAR_SVG_MAP: Record<ResolvedArchetype, React.FC<AvatarSvgProps>> = {
  fox:   FoxAvatar,
  bear:  BearAvatar,
  owl:   OwlAvatar,
  spark: SparkAvatar,
  cat:   CatAvatar,
  bot:   BotAvatar,
  sage:  SageAvatar,
};

export const ARCHETYPE_LABELS: Record<ResolvedArchetype, string> = {
  fox:   'Fox',
  bear:  'Bear',
  owl:   'Owl',
  spark: 'Spark',
  cat:   'Cat',
  bot:   'Bot',
  sage:  'Sage',
};
