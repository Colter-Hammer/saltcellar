import { useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

export type CategoryGroup = components['schemas']['CategoryGroup']
export type CategoryGroupKind = components['schemas']['CategoryGroupKind']
type CategoryList = components['schemas']['CategoryList']

const CATEGORIES = ['categories']

// Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
const JSON_HEADERS = { 'Content-Type': 'application/json' }

/** `GET /api/v1/categories`: every group in order with its categories, hidden ones included. */
export function useCategories() {
  return useQuery({
    queryKey: CATEGORIES,
    queryFn: () => call(client.GET('/api/v1/categories')),
  })
}

/**
 * Every change to the categories. Each one reloads the list afterwards, whether it worked or not,
 * and throws an ApiError when it didn't: a `validation_failed` names the `name` field.
 */
export function useCategoryEdits() {
  const queryClient = useQueryClient()
  async function edit(request: Promise<unknown>): Promise<void> {
    try {
      await request
    } finally {
      await queryClient.invalidateQueries({ queryKey: CATEGORIES })
    }
  }
  return {
    /** Writes the starter categories and the always-review payees into an empty list. */
    applyStarter: () =>
      edit(call(client.POST('/api/v1/categories/starter', { headers: JSON_HEADERS }))),
    addGroup: (name: string, kind: CategoryGroupKind) =>
      edit(call(client.POST('/api/v1/category-groups', { body: { name, kind } }))),
    /** Renames the group, or hides it or shows it again. */
    changeGroup: (id: number, change: { name?: string; hidden?: boolean }) =>
      edit(
        call(
          client.PATCH('/api/v1/category-groups/{id}', { params: { path: { id } }, body: change }),
        ),
      ),
    /** Deletes an empty group; one holding categories answers `group_not_empty`. */
    deleteGroup: (id: number) =>
      edit(
        call(
          client.DELETE('/api/v1/category-groups/{id}', {
            params: { path: { id } },
            headers: JSON_HEADERS,
          }),
        ),
      ),
    addCategory: (groupId: number, name: string) =>
      edit(call(client.POST('/api/v1/categories', { body: { group_id: groupId, name } }))),
    /** Renames the category, or hides it or shows it again. */
    changeCategory: (id: number, change: { name?: string; hidden?: boolean }) =>
      edit(
        call(client.PATCH('/api/v1/categories/{id}', { params: { path: { id } }, body: change })),
      ),
    deleteCategory: (id: number) =>
      edit(
        call(
          client.DELETE('/api/v1/categories/{id}', {
            params: { path: { id } },
            headers: JSON_HEADERS,
          }),
        ),
      ),
    /** Saves `groups`' order, shown at once; a category listed under another group moves to it. */
    order: (groups: CategoryGroup[]) => {
      queryClient.setQueryData<CategoryList>(CATEGORIES, { groups })
      const body = {
        groups: groups.map((group) => ({
          id: group.id,
          categories: group.categories.map((category) => category.id),
        })),
      }
      return edit(call(client.PUT('/api/v1/categories/order', { body })))
    },
  }
}
