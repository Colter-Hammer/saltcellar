import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createApp, defineComponent, h } from 'vue'
import { ApiError } from './client'
import { useFeedAccountDecisions } from './connectionAccounts'

function answer(status: number, body: string) {
  vi.stubGlobal(
    'fetch',
    vi.fn(() =>
      Promise.resolve(
        new Response(status === 204 ? null : body, {
          status,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    ),
  )
}

function sent(): Request {
  const [request] = vi.mocked(fetch).mock.calls[0] ?? []
  if (!(request instanceof Request)) throw new Error('nothing was sent')
  return request
}

function inApp<T>(use: () => T): { result: T; queryClient: QueryClient } {
  const queryClient = new QueryClient()
  let result: T | undefined
  const app = createApp(
    defineComponent({
      setup() {
        result = use()
        return () => h('div')
      },
    }),
  )
  app.use(VueQueryPlugin, { queryClient })
  app.mount(document.createElement('div'))
  if (result === undefined) throw new Error('setup did not run')
  return { result, queryClient }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useFeedAccountDecisions', () => {
  it('tracks one and reloads the prompt and the accounts', async () => {
    answer(200, '{"account_id":4}')
    const { result, queryClient } = inApp(useFeedAccountDecisions)
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    const how = { role: 'budgeted' as const, balances_only: false, same_as_account_id: 2 }
    expect(await result.track('ACT-fake-1', how)).toEqual({ account_id: 4 })
    expect(sent().method).toBe('POST')
    expect(new URL(sent().url).pathname).toBe('/api/v1/connection-accounts/ACT-fake-1/track')
    expect(await sent().json()).toEqual(how)
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['connection-accounts'] })
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['accounts'] })
  })

  it('ignores one with a JSON POST, and throws why a decision was refused', async () => {
    answer(204, '')
    const { result } = inApp(useFeedAccountDecisions)
    await result.ignore('ACT-fake-2')
    expect(new URL(sent().url).pathname).toBe('/api/v1/connection-accounts/ACT-fake-2/ignore')
    expect(sent().headers.get('Content-Type')).toBe('application/json')

    answer(422, '{"error":{"code":"foreign_currency","message":"It reports CAD."}}')
    const how = { role: 'tracking' as const, balances_only: true }
    const error: unknown = await result.track('ACT-fake-3', how).catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 422, code: 'foreign_currency' })
  })
})
