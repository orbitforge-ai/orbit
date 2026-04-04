import { useQuery, useQueryClient } from '@tanstack/react-query';
import { llmApi } from '../api/llm';

export function useApiKeyStatus(provider: string) {
  return useQuery({
    queryKey: ['api-key-status', provider],
    queryFn: () => llmApi.hasApiKey(provider),
    staleTime: 30_000,
  });
}

export function useInvalidateApiKeys() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: ['api-key-status'] });
}
