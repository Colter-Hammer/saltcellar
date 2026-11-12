import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createApp, defineComponent, h } from 'vue'
import { ApiError } from './client'
import { useDropped, useFlagActions } from './syncFlags'

function answer(status: number, body: string) {
  vi.stubGlobal(
    'fetch',
    vi.fn(() =>
      Promise.resolve(
        new Response(body, { status, headers: { 'Content-Type': 'application/json' } }),
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

describe('useFlagActions', () => {
  it('answers a flag and reloads the transactions, the dropped rows and the accounts', async () => {
    answer(200, '{"id":7}')
    const { result, queryClient } = inApp(useFlagActions)
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    expect(await result.resolve(7, 'merge')).toEqual({ id: 7 })
    expect(sent().method).toBe('POST')
    expect(new URL(sent().url).pathname).toBe('/api/v1/transactions/7/resolve')
    expect(await sent().json()).toEqual({ action: 'merge' })
    for (const key of ['transactions', 'dropped', 'accounts']) {
      expect(invalidate).toHaveBeenCalledWith({ queryKey: [key] })
    }
  })

  it('fixes a split, and restores a dropped row with a JSON POST', async () => {
    answer(200, '{"id":7}')
    const { result } = inApp(useFlagActions)
    const lines = [{ category_id: 1, amount_minor: -500 }]
    await result.fixSplit(7, lines)
    expect(sent().method).toBe('PUT')
    expect(new URL(sent().url).pathname).toBe('/api/v1/transactions/7/splits')
    expect(await sent().json()).toEqual({ lines })

    answer(200, '{"id":9}')
    expect(await result.restore(3)).toEqual({ id: 9 })
    expect(new URL(sent().url).pathname).toBe('/api/v1/dropped/3/restore')
    expect(sent().headers.get('Content-Type')).toBe('application/json')
  })

  it('throws why an action was refused', async () => {
    answer(409, '{"error":{"code":"not_flagged","message":"Not any more."}}')
    const { result } = inApp(useFlagActions)
    const error: unknown = await result.resolve(7, 'keep').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 409, code: 'not_flagged' })
  })
})

describe('useDropped', () => {
  it('reads the rows', async () => {
    answer(200, '{"rows":[{"id":3}]}')
    const { result } = inApp(() => useDropped(true))
    await vi.waitFor(() => {
      expect(result.data.value).toEqual([{ id: 3 }])
    })
    expect(new URL(sent().url).pathname).toBe('/api/v1/dropped')
  })
})
