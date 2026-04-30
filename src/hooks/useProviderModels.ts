import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { llmApi, ModelOption } from '../api/llm';
import { DEFAULT_MODEL_BY_PROVIDER, MODEL_OPTIONS } from '../constants/providers';

export function useAllProviderModelOptions(): Record<string, ModelOption[]> {
  const { data: vercelModels } = useQuery({
    queryKey: ['vercel-gateway-models'],
    queryFn: () => llmApi.listVercelGatewayModels(),
    staleTime: 10 * 60 * 1000,
    retry: 1,
  });

  return useMemo(
    () => ({
      ...MODEL_OPTIONS,
      vercel: vercelModels && vercelModels.length > 0 ? vercelModels : MODEL_OPTIONS.vercel,
    }),
    [vercelModels]
  );
}

export function defaultModelForProvider(provider: string, models: ModelOption[]): string {
  return DEFAULT_MODEL_BY_PROVIDER[provider] ?? models[0]?.value ?? '';
}
