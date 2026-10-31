import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { call, client } from './client'
import type { components } from './generated/schema'

export type UndecidedGroup = components['schemas']['UndecidedGroup']

const UNDECIDED = ['undecided']

/**
 * `GET /api/v1/undecided`: the From Actual, undecided rows grouped by their Actual category. An
 * empty list means nothing is left to decide. It also decides whether the navigation lists the
 * screen, so a lost session isn't sent to the log-in page by this call alone *(agent)*.
 */
export function useUndecided() {
  return useQuery({
    queryKey: UNDECIDED,
    queryFn: async (): Promise<UndecidedGroup[]> =>
      (await call(client.GET('/api/v1/undecided'), { quiet: true })).groups,
  })
}

/** One Actual category's undecided rows, a page at a time, once `enabled`. */
export function useUndecidedRows(
  actualCategory: MaybeRefOrGetter<string>,
  enabled: MaybeRefOrGetter<boolean>,
) {
  return useInfiniteQuery({
    queryKey: computed(() => ['transactions', 'list', 'undecided', toValue(actualCategory)]),
    enabled: computed(() => toValue(enabled)),
    queryFn: ({ pageParam }) =>
      call(
        client.GET('/api/v1/transactions', {
          params: {
            query: {
              undecided: true,
              actual_category: toValue(actualCategory),
              ...(pageParam !== null && { after: pageParam }),
            },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
  })
}

/**
 * Setting a category on undecided rows: one row, or every row of an Actual category. Either way
 * the lists and details reload afterwards, whether it worked or not, and a refusal throws an
 * ApiError.
 */
export function useDecide() {
  const queryClient = useQueryClient()
  async function reload() {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: UNDECIDED }),
      queryClient.invalidateQueries({ queryKey: ['transactions'] }),
    ])
  }
  const one = useMutation({
    mutationFn: ({ id, categoryId }: { id: number; categoryId: number }) =>
      call(
        client.PUT('/api/v1/transactions/{id}/category', {
          params: { path: { id } },
          body: { category_id: categoryId },
        }),
      ),
    onSettled: reload,
  })
  const group = useMutation({
    mutationFn: ({ actualCategory, categoryId }: { actualCategory: string; categoryId: number }) =>
      call(
        client.POST('/api/v1/undecided/move', {
          body: { actual_category: actualCategory, category_id: categoryId },
        }),
      ),
    onSettled: reload,
  })
  return { one, group }
}
