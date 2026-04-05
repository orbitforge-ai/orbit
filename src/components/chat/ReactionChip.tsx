interface ReactionChipProps {
  emoji: string;
  isNew?: boolean;
}

export function ReactionChip({ emoji, isNew }: ReactionChipProps) {
  return (
    <span
      className={`inline-flex items-center justify-center w-7 h-7 rounded-full bg-surface border border-edge text-sm shadow-sm ${isNew ? 'reaction-appear' : ''}`}
    >
      {emoji}
    </span>
  );
}
