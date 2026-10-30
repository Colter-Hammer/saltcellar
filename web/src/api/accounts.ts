import { useQuery } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { call, client } from './client'
import type { components } from './generated/schema'

export type Account = components['schemas']['Account']

/** `GET /api/v1/accounts`: every account in order with its balances, closed ones included. */
export function useAccounts() {
  return useQuery({
    queryKey: ['accounts'],
    queryFn: () => call(client.GET('/api/v1/accounts')),
  })
}

/** `GET /api/v1/accounts/{id}`: one account with its balances; a `not_found` ApiError if none. */
export function useAccount(id: MaybeRefOrGetter<number>) {
  return useQuery({
    queryKey: computed(() => ['accounts', toValue(id)]),
    queryFn: () =>
      call(client.GET('/api/v1/accounts/{id}', { params: { path: { id: toValue(id) } } })),
    retry: false,
  })
}
