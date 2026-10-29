import { useQuery } from '@tanstack/vue-query'
import { call, client } from './client'

/** `GET /api/v1/household`: the household's currency and timezone. */
export function useHousehold() {
  return useQuery({
    queryKey: ['household'],
    queryFn: () => call(client.GET('/api/v1/household')),
    staleTime: Infinity,
  })
}
