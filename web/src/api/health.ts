import { useQuery } from '@tanstack/vue-query'
import { call, client } from './client'

/** `GET /api/v1/health`: resolves while the server and its database answer. */
export function getHealth() {
  return call(client.GET('/api/v1/health'))
}

/** The server's health, polled: every 30 s while it answers, every 5 s while it doesn't. */
export function useHealth() {
  return useQuery({
    queryKey: ['health'],
    queryFn: getHealth,
    retry: false,
    refetchInterval: (query) => (query.state.status === 'error' ? 5_000 : 30_000),
  })
}
