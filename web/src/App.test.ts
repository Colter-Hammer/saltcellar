import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory } from 'vue-router'
import App from './App.vue'
import { createAppRouter } from './router'

function serverUp() {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.resolve(Response.json({ status: 'ok' }))),
  )
}

function serverDown() {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
  )
}

beforeEach(serverUp)

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

async function open(path: string) {
  const router = createAppRouter(createMemoryHistory())
  await router.push(path)
  const queryClient = new QueryClient()
  const wrapper = mount(App, { global: { plugins: [router, [VueQueryPlugin, { queryClient }]] } })
  await flushPromises()
  return { router, wrapper }
}

function links(wrapper: Awaited<ReturnType<typeof open>>['wrapper'], nav: string) {
  return wrapper.get(`nav[aria-label="${nav}"]`).findAll('a')
}

describe('the app shell', () => {
  it('lands on Transactions', async () => {
    const { router, wrapper } = await open('/')
    expect(router.currentRoute.value.path).toBe('/transactions')
    expect(wrapper.get('h1').text()).toBe('Transactions')
  })

  it('has the milestone 1 tabs on phones', async () => {
    const { wrapper } = await open('/')
    expect(links(wrapper, 'Tabs').map((a) => a.text())).toEqual([
      'Transactions',
      'Accounts',
      'More',
    ])
  })

  it('lists every milestone 1 screen in the desktop sidebar', async () => {
    const { wrapper } = await open('/')
    expect(links(wrapper, 'Sidebar').map((a) => a.text())).toEqual([
      'Transactions',
      'Accounts',
      'Categories',
      'Import from Actual',
      'Settings',
    ])
  })

  it('lists the rest under More', async () => {
    const { wrapper } = await open('/more')
    expect(wrapper.get('h1').text()).toBe('More')
    expect(
      wrapper
        .get('main')
        .findAll('a')
        .map((a) => a.text()),
    ).toEqual(['Categories', 'Import from Actual', 'Settings'])
  })

  it('marks More as the current tab on the screens it lists', async () => {
    const { wrapper } = await open('/categories')
    expect(wrapper.get('h1').text()).toBe('Categories')
    const current = links(wrapper, 'Tabs').filter((a) => a.attributes('aria-current') === 'page')
    expect(current.map((a) => a.text())).toEqual(['More'])
  })

  it('moves between screens from the tabs', async () => {
    const { router, wrapper } = await open('/transactions')
    const accounts = links(wrapper, 'Tabs').find((a) => a.text() === 'Accounts')
    await accounts?.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/accounts')
    expect(wrapper.get('h1').text()).toBe('Accounts')
  })

  it('says so for an address that is no screen', async () => {
    const { wrapper } = await open('/transactions/whatever')
    expect(wrapper.get('h1').text()).toBe('Not found')
  })
})

describe('the server banner', () => {
  it("isn't there while the server answers", async () => {
    const { wrapper } = await open('/')
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('says so when the server is unreachable', async () => {
    serverDown()
    const { wrapper } = await open('/')
    expect(wrapper.get('[role="alert"]').text()).toContain("Can't reach the server")
  })

  it('goes away once the server answers again', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
    serverDown()
    const { wrapper } = await open('/')
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)
    serverUp()
    await vi.advanceTimersByTimeAsync(5_000)
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })
})
