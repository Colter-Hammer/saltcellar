import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { call, client } from './client'
import type { TransactionFilter } from '@/lib/transactions'
import type { components, operations } from './generated/schema'

export type TransactionRow = components['schemas']['TransactionRow']
export type Transaction = components['schemas']['Transaction']
type Tag = components['schemas']['Tag']
type ListQuery = NonNullable<operations['getTransactions']['parameters']['query']>

/** The search and filters as the API's query takes them. */
function listQuery(filter: TransactionFilter): ListQuery {
  const { account, tag, undecided, ...text } = filter
  return {
    ...text,
    ...(account !== undefined && { account: Number(account) }),
    ...(tag !== undefined && { tag: Number(tag) }),
    ...(undecided !== undefined && { undecided: undecided === 'true' }),
  }
}

/**
 * `GET /api/v1/transactions`, a page at a time: the ones `filter` picks, newest first by date then
 * id, each page starting after the last one's cursor.
 */
export function useTransactions(filter: MaybeRefOrGetter<TransactionFilter> = {}) {
  return useInfiniteQuery({
    queryKey: computed(() => ['transactions', 'list', toValue(filter)]),
    queryFn: ({ pageParam }) =>
      call(
        client.GET('/api/v1/transactions', {
          params: {
            query: {
              ...listQuery(toValue(filter)),
              ...(pageParam !== null && { after: pageParam }),
            },
          },
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

/** `GET /api/v1/tags`: every tag, by name, for the tag filter. */
export function useTags() {
  return useQuery({
    queryKey: ['tags'],
    queryFn: async (): Promise<Tag[]> => (await call(client.GET('/api/v1/tags'))).tags,
  })
}

export type ManualEntry = components['schemas']['ManualEntry']
type Payee = components['schemas']['Payee']

/** `GET /api/v1/payees`: every payee, by name, for the entry form. */
export function usePayees() {
  return useQuery({
    queryKey: ['payees'],
    queryFn: async (): Promise<Payee[]> => (await call(client.GET('/api/v1/payees'))).payees,
  })
}

/**
 * Entering, editing and deleting manual transactions. Each reloads the transactions, payees, tags
 * and accounts afterwards, whether it worked or not, and throws an ApiError when it didn't: a
 * `validation_failed` names the fields.
 */
export function useManualEdits() {
  const queryClient = useQueryClient()
  async function edit<T>(request: Promise<T>): Promise<T> {
    try {
      return await request
    } finally {
      await Promise.all(
        ['transactions', 'payees', 'tags', 'accounts'].map((key) =>
          queryClient.invalidateQueries({ queryKey: [key] }),
        ),
      )
    }
  }
  return {
    /** `POST /api/v1/transactions`: enters one; resolves to it. */
    create: (entry: ManualEntry) =>
      edit(call(client.POST('/api/v1/transactions', { body: entry }))),
    /** `PUT /api/v1/transactions/{id}`: replaces every field; resolves to it. */
    change: (id: number, entry: ManualEntry) =>
      edit(
        call(client.PUT('/api/v1/transactions/{id}', { params: { path: { id } }, body: entry })),
      ),
    /** `DELETE /api/v1/transactions/{id}`, marked JSON as every changing request must be. */
    remove: (id: number) =>
      edit(
        call(
          client.DELETE('/api/v1/transactions/{id}', {
            params: { path: { id } },
            headers: { 'Content-Type': 'application/json' },
          }),
        ),
      ),
  }
}
