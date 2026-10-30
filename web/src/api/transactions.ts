import { useInfiniteQuery, useQuery } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { call, client } from './client'
import type { components } from './generated/schema'

export type TransactionRow = components['schemas']['TransactionRow']
export type Transaction = components['schemas']['Transaction']

/**
 * `GET /api/v1/transactions`, a page at a time: newest first by date then id, each page starting
 * after the last one's cursor.
 */
export function useTransactions() {
  return useInfiniteQuery({
    queryKey: ['transactions'],
    queryFn: ({ pageParam }) =>
      call(
        client.GET('/api/v1/transactions', {
          params: { query: pageParam === null ? {} : { after: pageParam } },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
  })
}

/** `GET /api/v1/transactions/{id}`: one transaction in full; a `not_found` ApiError if none. */
export function useTransaction(id: MaybeRefOrGetter<number>) {
  return useQuery({
    queryKey: computed(() => ['transactions', toValue(id)]),
    queryFn: () =>
      call(client.GET('/api/v1/transactions/{id}', { params: { path: { id: toValue(id) } } })),
    retry: false,
  })
}
