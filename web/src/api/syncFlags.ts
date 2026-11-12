import { useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { call, client } from './client'
import type { components } from './generated/schema'

export type FlagAction = components['schemas']['FlagAction']
export type SplitLineEntry = components['schemas']['SplitLineEntry']
export type DroppedRow = components['schemas']['DroppedRow']

const DROPPED = ['dropped']

/**
 * Answering a row's sync flag and fixing a split (docs/spec/m2-bank-data.md#sync-health-in-the-ui),
 * and restoring a row the bank dropped. Each reloads the transactions, the dropped rows and the
 * accounts afterwards, whether it worked or not, and throws an ApiError when it didn't. Each
 * resolves to the transaction to show after.
 */
export function useFlagActions() {
  const queryClient = useQueryClient()
  async function edit<T>(request: Promise<T>): Promise<T> {
    try {
      return await request
    } finally {
      await Promise.all(
        [['transactions'], DROPPED, ['accounts']].map((queryKey) =>
          queryClient.invalidateQueries({ queryKey }),
        ),
      )
    }
  }
  return {
    /** `POST /api/v1/transactions/{id}/resolve`: the flag's action; the flag goes. */
    resolve: (id: number, action: FlagAction) =>
      edit(
        call(
          client.POST('/api/v1/transactions/{id}/resolve', {
            params: { path: { id } },
            body: { action },
          }),
        ),
      ),
    /** `PUT /api/v1/transactions/{id}/splits`: new lines adding up to the amount. */
    fixSplit: (id: number, lines: SplitLineEntry[]) =>
      edit(
        call(
          client.PUT('/api/v1/transactions/{id}/splits', {
            params: { path: { id } },
            body: { lines },
          }),
        ),
      ),
    /** `POST /api/v1/dropped/{id}/restore`, marked JSON as every changing request must be. */
    restore: (id: number) =>
      edit(
        call(
          client.POST('/api/v1/dropped/{id}/restore', {
            params: { path: { id } },
            headers: { 'Content-Type': 'application/json' },
          }),
        ),
      ),
  }
}

/** `GET /api/v1/dropped`: Recently dropped by bank, once `enabled`. */
export function useDropped(enabled: MaybeRefOrGetter<boolean>) {
  return useQuery({
    queryKey: DROPPED,
    enabled: computed(() => toValue(enabled)),
    queryFn: async (): Promise<DroppedRow[]> => (await call(client.GET('/api/v1/dropped'))).rows,
  })
}
