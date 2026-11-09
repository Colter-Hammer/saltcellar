import { useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

export type FeedAccount = components['schemas']['FeedAccount']
export type TrackFeedAccount = components['schemas']['TrackFeedAccount']
export type ConnectionAccounts = components['schemas']['ConnectionAccounts']

/**
 * `GET /api/v1/connection-accounts`: the new accounts prompt, the accounts the bank connection
 * reports that wait to be tracked, the ignored ones, and the ones refused for their currency.
 */
export function useConnectionAccounts() {
  return useQuery({
    queryKey: ['connection-accounts'],
    queryFn: () => call(client.GET('/api/v1/connection-accounts')),
  })
}

/**
 * Deciding on a feed account. Each reloads the prompt and the accounts afterwards, whether it
 * worked or not, and throws an ApiError when it didn't (`already_tracked`, `foreign_currency`,
 * `not_found`).
 */
export function useFeedAccountDecisions() {
  const queryClient = useQueryClient()
  async function decide<T>(request: Promise<T>): Promise<T> {
    try {
      return await request
    } finally {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['connection-accounts'] }),
        queryClient.invalidateQueries({ queryKey: ['accounts'] }),
      ])
    }
  }
  return {
    /** `POST …/{id}/track`: creates its account, or moves `same_as_account_id` onto it. */
    track: (externalAccountId: string, track: TrackFeedAccount) =>
      decide(
        call(
          client.POST('/api/v1/connection-accounts/{external_account_id}/track', {
            params: { path: { external_account_id: externalAccountId } },
            body: track,
          }),
        ),
      ),
    /** `POST …/{id}/ignore`: moves it to Ignored. */
    ignore: (externalAccountId: string) =>
      decide(
        call(
          client.POST('/api/v1/connection-accounts/{external_account_id}/ignore', {
            params: { path: { external_account_id: externalAccountId } },
            // Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
            headers: { 'Content-Type': 'application/json' },
          }),
        ),
      ),
  }
}
