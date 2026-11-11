import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createApp, defineComponent, h } from 'vue'
import { ApiError } from './client'
import { useClaimSetupToken, useDisconnectBank, useSyncNow } from './bank'

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

/** Runs `use` inside a component with the query client, and resolves to what it returned. */
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

const connected = {
  secret_key_set: true,
  connection: { label: 'SimpleFIN Bridge', status: 'ok', paused: false, last_sync: null },
}

describe('useClaimSetupToken', () => {
  it('sends the token and keeps the connection it answers', async () => {
    answer(200, JSON.stringify(connected))
    const { result, queryClient } = inApp(useClaimSetupToken)
    expect(await result.mutateAsync('c2V0dXA=')).toEqual(connected)
    expect(sent().method).toBe('PUT')
    expect(new URL(sent().url).pathname).toBe('/api/v1/bank-connection')
    expect(await sent().json()).toEqual({ setup_token: 'c2V0dXA=' })
    expect(queryClient.getQueryData(['bank-connection'])).toEqual(connected)
  })

  it('throws why a token is refused', async () => {
    answer(
      409,
      '{"error":{"code":"secret_key_missing","message":"Bank sync needs BUDGET_SECRET_KEY."}}',
    )
    const { result } = inApp(useClaimSetupToken)
    const error: unknown = await result.mutateAsync('c2V0dXA=').catch((e: unknown) => e)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 409, code: 'secret_key_missing' })
  })
})

describe('useDisconnectBank', () => {
  it('sends a JSON DELETE and reloads everything', async () => {
    answer(204, '')
    const { result, queryClient } = inApp(useDisconnectBank)
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    await result.mutateAsync()
    expect(sent().method).toBe('DELETE')
    expect(sent().headers.get('Content-Type')).toBe('application/json')
    expect(invalidate).toHaveBeenCalled()
  })
})

describe('useSyncNow', () => {
  const run = {
    trigger: 'manual',
    started_at: '2026-10-28T09:00:00Z',
    finished_at: '2026-10-28T09:00:02Z',
    requests: 1,
    added: 2,
    updated: 1,
    error: null,
  }

  it('sends a JSON POST, answers the run and reloads the rows', async () => {
    answer(200, JSON.stringify(run))
    const { result, queryClient } = inApp(useSyncNow)
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
    expect(await result.mutateAsync()).toEqual(run)
    expect(sent().method).toBe('POST')
    expect(new URL(sent().url).pathname).toBe('/api/v1/bank-connection/sync')
    expect(sent().headers.get('Content-Type')).toBe('application/json')
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['transactions'] })
  })

  it('throws why it can’t sync', async () => {
    answer(409, '{"error":{"code":"sync_paused","message":"Bank sync is paused."}}')
    const { result } = inApp(useSyncNow)
    const error: unknown = await result.mutateAsync().catch((e: unknown) => e)
    expect(error).toMatchObject({ status: 409, code: 'sync_paused' })
  })
})
