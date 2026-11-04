import { useQuery, useQueryClient } from '@tanstack/vue-query'
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

type AccountList = components['schemas']['AccountList']
export type AccountChange = components['schemas']['AccountChange']

/**
 * Every change to the accounts. Each one reloads them afterwards, whether it worked or not, and
 * throws an ApiError when it didn't: a `validation_failed` names the `name` field.
 */
export function useAccountEdits() {
  const queryClient = useQueryClient()
  async function edit<T>(request: Promise<T>): Promise<T> {
    try {
      return await request
    } finally {
      await queryClient.invalidateQueries({ queryKey: ['accounts'] })
    }
  }
  return {
    /** `PATCH /api/v1/accounts/{id}`: changes what's listed in `change`, nothing else. */
    change: (id: number, change: AccountChange) =>
      edit(call(client.PATCH('/api/v1/accounts/{id}', { params: { path: { id } }, body: change }))),
    /** Saves `accounts`' order, shown at once. */
    order: (accounts: Account[]) => {
      queryClient.setQueryData<AccountList>(['accounts'], { accounts })
      const body = { accounts: accounts.map((account) => account.id) }
      return edit(call(client.PUT('/api/v1/accounts/order', { body })))
    },
  }
}
