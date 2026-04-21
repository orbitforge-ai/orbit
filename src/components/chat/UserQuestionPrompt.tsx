import { useMemo, useState } from 'react';
import { HelpCircle, Check } from 'lucide-react';
import { chatApi } from '../../api/chat';
import { Textarea } from '../ui';

interface UserQuestionPromptProps {
  requestId: string;
  question: string;
  choices?: string[];
  allowCustom: boolean;
  multiSelect: boolean;
  context?: string;
}

export function UserQuestionPrompt({
  requestId,
  question,
  choices,
  allowCustom,
  multiSelect,
  context,
}: UserQuestionPromptProps) {
  const [customValue, setCustomValue] = useState('');
  const [selectedChoices, setSelectedChoices] = useState<string[]>([]);
  const [resolvedResponse, setResolvedResponse] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const canSubmit = useMemo(() => {
    if (submitting || resolvedResponse) return false;
    if (multiSelect) {
      return selectedChoices.length > 0 || (allowCustom && customValue.trim().length > 0);
    }
    if (choices && choices.length > 0) {
      return selectedChoices.length > 0 || (allowCustom && customValue.trim().length > 0);
    }
    return customValue.trim().length > 0;
  }, [allowCustom, choices, customValue, multiSelect, resolvedResponse, selectedChoices, submitting]);

  const handleSubmit = async (presetChoice?: string) => {
    const response = buildResponse({
      presetChoice,
      customValue,
      selectedChoices,
      multiSelect,
    });
    if (!response) return;

    setSubmitting(true);
    try {
      await chatApi.respondToUserQuestion(requestId, response);
      setResolvedResponse(response);
    } finally {
      setSubmitting(false);
    }
  };

  const toggleChoice = (choice: string) => {
    if (!multiSelect) {
      setSelectedChoices([choice]);
      return;
    }
    setSelectedChoices((current) =>
      current.includes(choice) ? current.filter((item) => item !== choice) : [...current, choice]
    );
  };

  if (resolvedResponse) {
    return (
      <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/5 px-3 py-2">
        <div className="flex items-center gap-2 text-xs text-emerald-300">
          <Check size={12} className="shrink-0" />
          <span className="font-medium">User responded</span>
        </div>
        <div className="mt-1 text-xs text-secondary whitespace-pre-wrap break-words">
          {resolvedResponse}
        </div>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-blue-500/30 bg-blue-500/5 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2">
        <HelpCircle size={14} className="text-blue-400 shrink-0" />
        <span className="text-xs font-medium text-white">Question For You</span>
      </div>
      <div className="px-3 pb-3 space-y-3">
        <p className="text-sm text-secondary whitespace-pre-wrap break-words">{question}</p>

        {context && (
          <pre className="rounded-md border border-white/10 bg-background/60 p-2 text-[11px] text-muted whitespace-pre-wrap break-words max-h-40 overflow-y-auto">
            {context}
          </pre>
        )}

        {choices && choices.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {choices.map((choice) => {
              const selected = selectedChoices.includes(choice);
              return (
                <button
                  key={choice}
                  type="button"
                  onClick={() => toggleChoice(choice)}
                  className={`rounded-full border px-3 py-1.5 text-xs transition-colors ${
                    selected
                      ? 'border-blue-400 bg-blue-500/15 text-blue-200'
                      : 'border-edge bg-background text-secondary hover:text-white hover:border-edge-hover'
                  }`}
                >
                  {choice}
                </button>
              );
            })}
          </div>
        )}

        {allowCustom && (
          <Textarea
            value={customValue}
            onChange={(event) => setCustomValue(event.target.value)}
            rows={3}
            placeholder={choices?.length ? 'Add a custom response' : 'Type your answer'}
            className="bg-background px-3 py-2"
          />
        )}

        {choices && !multiSelect && !allowCustom && (
          <div className="flex flex-wrap gap-2">
            {choices.map((choice) => (
              <button
                key={`quick-${choice}`}
                type="button"
                onClick={() => handleSubmit(choice)}
                disabled={submitting}
                className="rounded-md bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500 disabled:opacity-60"
              >
                {choice}
              </button>
            ))}
          </div>
        )}

        {(allowCustom || multiSelect || !choices || choices.length === 0) && (
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => handleSubmit()}
              disabled={!canSubmit}
              className="rounded-md bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500 disabled:opacity-60"
            >
              {submitting ? 'Submitting...' : 'Submit'}
            </button>
            {multiSelect && selectedChoices.length > 0 && (
              <span className="text-[11px] text-muted">
                {selectedChoices.length} choice{selectedChoices.length === 1 ? '' : 's'} selected
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function buildResponse({
  presetChoice,
  customValue,
  selectedChoices,
  multiSelect,
}: {
  presetChoice?: string;
  customValue: string;
  selectedChoices: string[];
  multiSelect: boolean;
}) {
  if (presetChoice) return presetChoice;

  const trimmedCustom = customValue.trim();
  if (multiSelect) {
    const values = [...selectedChoices];
    if (trimmedCustom) values.push(trimmedCustom);
    return values.length > 0 ? JSON.stringify(values) : '';
  }

  if (selectedChoices.length > 0) {
    return selectedChoices[0];
  }

  return trimmedCustom;
}
