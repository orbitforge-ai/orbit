import { useEffect, useRef, useState, useCallback } from 'react';
import {
  onAgentLlmChunk,
  onAgentContentBlock,
  onAgentToolResult,
  onAgentIteration,
} from '../../events/runEvents';
import { AvatarState } from './types';

export function useAvatarState(streamId: string | null): {
  state: AvatarState;
  forceThinking: () => void;
} {
  const [state, setState] = useState<AvatarState>({ phase: 'idle' });
  // Accumulate streaming text for the speech bubble
  const textRef = useRef('');

  // Reset when stream changes (new session, draft mode, etc.)
  useEffect(() => {
    setState({ phase: 'idle' });
    textRef.current = '';
  }, [streamId]);

  const forceThinking = useCallback(() => {
    textRef.current = '';
    setState({ phase: 'thinking' });
  }, []);

  useEffect(() => {
    if (!streamId) return;

    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onAgentLlmChunk((payload) => {
        if (payload.runId !== streamId) return;
        textRef.current = (textRef.current + payload.delta).slice(-120);
        setState({ phase: 'speaking', text: textRef.current });
      })
    );

    unsubs.push(
      onAgentContentBlock((payload) => {
        if (payload.runId !== streamId) return;
        if (
          (payload.blockType === 'tool_use' || payload.blockType === 'tool_input_delta') &&
          'name' in payload.block
        ) {
          setState({ phase: 'using-tool', toolName: payload.block.name });
          return;
        }
        if (payload.blockType === 'thinking_delta' || payload.blockType === 'thinking') {
          textRef.current = '';
          setState({ phase: 'thinking' });
        }
      })
    );

    unsubs.push(
      onAgentToolResult((payload) => {
        if (payload.runId !== streamId) return;
        textRef.current = '';
        setState({ phase: 'thinking' });
      })
    );

    unsubs.push(
      onAgentIteration((payload) => {
        if (payload.runId !== streamId) return;
        if (payload.action === 'finished') {
          textRef.current = '';
          setState({ phase: 'idle' });
        } else if (payload.action === 'llm_call') {
          setState((prev) =>
            prev.phase === 'idle' ? { phase: 'thinking' } : prev
          );
        }
      })
    );

    return () => {
      unsubs.forEach((p) => p.then((unsub) => unsub()).catch(() => {}));
    };
  }, [streamId]);

  return { state, forceThinking };
}
