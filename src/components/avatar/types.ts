import { AvatarArchetype } from '../../types';

export type { AvatarArchetype };

export type ResolvedArchetype = Exclude<AvatarArchetype, 'auto'>;

export type AvatarPhase = 'idle' | 'thinking' | 'speaking' | 'using-tool';

export type AvatarState =
  | { phase: 'idle' }
  | { phase: 'thinking' }
  | { phase: 'speaking'; text: string }
  | { phase: 'using-tool'; toolName: string };

export interface AvatarSvgProps {
  size?: number;
  className?: string;
}
