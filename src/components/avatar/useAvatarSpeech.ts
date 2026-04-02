import { useEffect, useRef } from 'react';
import { AgentIdentityConfig } from '../../types';
import { AvatarState } from './types';

export function useAvatarSpeech(
  state: AvatarState,
  identity: AgentIdentityConfig | undefined,
  enabled: boolean
): void {
  const lastSpokenRef = useRef('');

  useEffect(() => {
    if (!enabled || !identity) return;
    if (typeof window === 'undefined' || !window.speechSynthesis) return;
    if (state.phase !== 'speaking') {
      // Cancel any ongoing speech when no longer speaking
      if (window.speechSynthesis.speaking) {
        window.speechSynthesis.cancel();
      }
      lastSpokenRef.current = '';
      return;
    }

    // Only speak new tail content (avoid re-speaking on every delta)
    const text = state.text;
    if (!text || text === lastSpokenRef.current) return;

    // Find only the new portion appended since last utterance
    const prev = lastSpokenRef.current;
    const newContent = text.startsWith(prev) ? text.slice(prev.length) : text;
    if (!newContent.trim()) return;

    // Speak in short chunks — only speak when we get a natural pause (period/newline)
    if (!newContent.match(/[.!?\n]/)) return;

    lastSpokenRef.current = text;
    const utterance = new SpeechSynthesisUtterance(newContent);
    utterance.rate = identity.directness > 60 ? 1.1 : 0.85;
    utterance.pitch = identity.warmth > 60 ? 1.1 : 0.9;
    utterance.volume = 0.8;
    window.speechSynthesis.speak(utterance);
  }, [state, identity, enabled]);

  // Cancel speech on unmount
  useEffect(() => {
    return () => {
      if (typeof window !== 'undefined' && window.speechSynthesis?.speaking) {
        window.speechSynthesis.cancel();
      }
    };
  }, []);
}
