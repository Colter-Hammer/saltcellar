import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { useTransactions } from './transactions'

/** Every request sent, as `METHOD path?query`. */
let sent: string[]

beforeEach(() => {
  sent = []
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) => {
      const { pathname, search } = new URL(request.url)
      sent.push(`${request.method} ${pathname}${search}`)
      return Promise.resolve(Response.json({ transactions: [], next_cursor: null }))
    }),
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('the list', () => {
  function list(filter: Record<string, string>) {
    const Host = defineComponent({
      setup() {
        useTransactions(filter)
        return () => null
      },
    })
    mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] } })
  }

  it('asks for Needs a look as the API names it', async () => {
    list({ q: 'cafe', needs_a_look: 'true' })
    await flushPromises()
    expect(sent).toEqual(['GET /api/v1/transactions?q=cafe&needs_a_look=true'])
  })

  it('asks for nothing while it shows Recently dropped by bank', async () => {
    list({ dropped: 'true' })
    await flushPromises()
    expect(sent).toEqual([])
  })
})
