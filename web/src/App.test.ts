import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory } from 'vue-router'
import App from './App.vue'
import { call, client } from './api/client'
import { createAppRouter } from './router'

/** What the fake server knows; each test starts from a finished setup, logged in. */
interface FakeServer {
  setupDone: boolean
  /** Whether the browser's session cookie is live. */
  loggedIn: boolean
  /** Failed log-ins in a row: the fifth pauses the next. */
  failures: number
  /** The setup code it printed when it started. */
  code: string
  /** What `POST /api/v1/setup` was sent. */
  finishedWith?: unknown
  /** Field errors it answers `POST /api/v1/setup` with, beside the password checks. */
  refuse?: Record<string, string>
}

let server: FakeServer

const SETUP_CODE = 'ABCD-EFGH'
const PASSWORD = 'correct horse battery'

function error(status: number, code: string, message: string) {
  return Response.json({ error: { code, message } }, { status })
}

async function answer(request: Request): Promise<Response> {
  const path = new URL(request.url).pathname
  if (path === '/api/v1/health') return Response.json({ status: 'ok' })
  if (path === '/api/v1/session') return session(request)
  if (path === '/api/v1/setup' && request.method === 'POST') {
    const body = (await request.json()) as Record<string, string>
    if (body.code !== server.code) {
      return error(
        403,
        'setup_code_wrong',
        "That isn't the setup code. Copy it from the app's log.",
      )
    }
    const fields: Record<string, string> = {}
    if ((body.password ?? '').length < 12) fields.password = 'Use at least 12 characters.'
    else if (body.password !== body.password_again)
      fields.password_again = "The passwords don't match."
    Object.assign(fields, server.refuse)
    if (Object.keys(fields).length > 0) {
      return Response.json(
        {
          error: { code: 'validation_failed', message: 'Some details need another look.', fields },
        },
        { status: 422 },
      )
    }
    server.finishedWith = body
    server.setupDone = true
    server.loggedIn = true
    return new Response(null, { status: 204 })
  }
  if (path === '/api/v1/setup') return Response.json({ done: server.setupDone })
  if (path === '/api/v1/household') {
    if (!server.loggedIn) return error(401, 'login_required', 'Log in to continue.')
    return Response.json({ currency: 'EUR', timezone: 'Europe/Berlin' })
  }
  if (path === '/api/v1/setup/currencies') return Response.json(['EUR', 'JPY', 'USD'])
  if (path === '/api/v1/setup/code') {
    const { code } = (await request.json()) as { code: string }
    if (code === server.code) return new Response(null, { status: 204 })
    return error(403, 'setup_code_wrong', "That isn't the setup code. Copy it from the app's log.")
  }
  return Response.json(
    { error: { code: 'not_found', message: "There's nothing at this address." } },
    { status: 404 },
  )
}

async function session(request: Request): Promise<Response> {
  if (request.method === 'GET') {
    if (!server.loggedIn) return error(401, 'login_required', 'Log in to continue.')
    return Response.json({ username: 'sam', display_name: 'Sam' })
  }
  if (request.method === 'DELETE') {
    server.loggedIn = false
    return new Response(null, { status: 204 })
  }
  if (server.failures >= 5) {
    return error(429, 'too_many_attempts', 'Too many attempts. Try again in 15 minutes.')
  }
  const { username, password } = (await request.json()) as Record<string, string>
  if (username?.trim().toLowerCase() !== 'sam' || password !== PASSWORD) {
    server.failures += 1
    return error(401, 'login_failed', 'Wrong username or password.')
  }
  server.failures = 0
  server.loggedIn = true
  return new Response(null, { status: 204 })
}

function serverUp() {
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) => answer(request)),
  )
}

function serverDown() {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
  )
}

// jsdom has no layout, so no scrolling; the picker scrolls its highlighted option into view.
Element.prototype.scrollIntoView = () => undefined

beforeEach(() => {
  server = { setupDone: true, loggedIn: true, failures: 0, code: SETUP_CODE }
  serverUp()
})

afterEach(() => {
  vi.unstubAllEnvs()
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

  it('offers the first steps while Transactions is empty', async () => {
    const { router, wrapper } = await open('/transactions')
    const links = wrapper.get('main').findAll('a')
    expect(links.map((a) => [a.text(), a.attributes('href')])).toEqual([
      ['Import from Actual', '/import'],
      ['Set up categories', '/categories'],
    ])
    await links[1]?.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/categories')
  })

  it("doesn't show the wizard once setup is done", async () => {
    const { router, wrapper } = await open('/setup')
    expect(router.currentRoute.value.path).toBe('/transactions')
    expect(wrapper.get('h1').text()).toBe('Transactions')
  })

  it('says so for an address that is no screen', async () => {
    const { wrapper } = await open('/transactions/whatever')
    expect(wrapper.get('h1').text()).toBe('Not found')
  })
})

describe('before setup', () => {
  it('shows only the wizard, whatever the address', async () => {
    server.setupDone = false
    const { router, wrapper } = await open('/accounts')
    expect(router.currentRoute.value.path).toBe('/setup')
    expect(wrapper.get('h1').text()).toBe('Set up Saltcellar')
    expect(wrapper.find('nav').exists()).toBe(false)
  })
})

type Wrapper = Awaited<ReturnType<typeof open>>['wrapper']

async function submit(wrapper: Wrapper, values: Record<string, string>) {
  for (const [name, value] of Object.entries(values)) {
    await wrapper.get(`[name="${name}"]`).setValue(value)
  }
  await wrapper.get('form').trigger('submit')
  await flushPromises()
}

/** Opens the searchable list on `field`, types `query` and taps the option reading `option`. */
async function choose(wrapper: Wrapper, field: string, query: string, option: string) {
  const input = wrapper.get(`#${field}`)
  await input.trigger('click')
  await input.setValue(query)
  await flushPromises()
  const match = wrapper.findAll('[role="option"]').find((o) => o.text() === option)
  if (match === undefined) throw new Error(`no option ${option} for ${query} in ${field}`)
  await match.trigger('click')
  await flushPromises()
}

/** The options the list on `field` shows once tapped, after typing `query`. */
async function optionsFor(wrapper: Wrapper, field: string, query?: string) {
  const input = wrapper.get(`#${field}`)
  await input.trigger('click')
  if (query !== undefined) await input.setValue(query)
  await flushPromises()
  return wrapper.findAll('[role="option"]').map((o) => o.text())
}

async function chooseHousehold(wrapper: Wrapper) {
  await choose(wrapper, 'currency', 'euro', 'Euro (EUR)')
  await choose(wrapper, 'timezone', 'berlin', 'Europe/Berlin')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
}

describe('the setup wizard', () => {
  beforeEach(() => {
    server.setupDone = false
  })

  it('refuses a wrong code and says where to find the right one', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: 'WXYZ-WXYZ' })
    expect(wrapper.get('form').text()).toContain("That isn't the setup code.")
    expect(wrapper.find('[name="code"]').exists()).toBe(true)
  })

  it('asks for the currency and timezone once the code is right', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    expect(wrapper.find('[name="code"]').exists()).toBe(false)
    expect(wrapper.find('[name="currency"]').exists()).toBe(true)
    expect(wrapper.find('[name="timezone"]').exists()).toBe(true)
  })

  it("suggests the browser locale's currency and the browser's timezone", async () => {
    vi.stubEnv('TZ', 'Asia/Tokyo')
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    expect(wrapper.get<HTMLInputElement>('#currency').element.value).toBe('US Dollar (USD)')
    expect(wrapper.get<HTMLInputElement>('#timezone').element.value).toBe('Asia/Tokyo')
  })

  it('lists every currency the server offers when the field is tapped, even with one chosen', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    expect(await optionsFor(wrapper, 'currency')).toEqual([
      'Euro (EUR)',
      'Japanese Yen (JPY)',
      'US Dollar (USD)',
    ])
  })

  it('finds currencies by name or code', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    expect(await optionsFor(wrapper, 'currency', 'yen')).toEqual(['Japanese Yen (JPY)'])
    expect(await optionsFor(wrapper, 'currency', 'eur')).toEqual(['Euro (EUR)'])
  })

  it('asks for the first login after the currency and timezone', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    await chooseHousehold(wrapper)
    for (const name of ['display_name', 'username', 'password', 'password_again']) {
      expect(wrapper.find(`[name="${name}"]`).exists(), name).toBe(true)
    }
    expect(wrapper.get<HTMLInputElement>('[name="password"]').element.type).toBe('password')
    expect(wrapper.get<HTMLInputElement>('[name="password_again"]').element.type).toBe('password')
  })

  it('finishes setup in one go and lands on Transactions', async () => {
    const { router, wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    await chooseHousehold(wrapper)
    await submit(wrapper, {
      display_name: 'Sam',
      username: 'sam',
      password: 'correct horse battery',
      password_again: 'correct horse battery',
    })
    expect(server.finishedWith).toEqual({
      code: SETUP_CODE,
      currency: 'EUR',
      timezone: 'Europe/Berlin',
      display_name: 'Sam',
      username: 'sam',
      password: 'correct horse battery',
      password_again: 'correct horse battery',
    })
    expect(router.currentRoute.value.path).toBe('/transactions')
    expect(wrapper.get('h1').text()).toBe('Transactions')
    expect(wrapper.find('nav[aria-label="Tabs"]').exists()).toBe(true)
  })

  async function toLogin() {
    const opened = await open('/setup')
    await submit(opened.wrapper, { code: SETUP_CODE })
    await chooseHousehold(opened.wrapper)
    return opened
  }

  const login = { display_name: 'Sam', username: 'sam' }

  it('refuses an 11-character password with an error on the field', async () => {
    const { router, wrapper } = await toLogin()
    await submit(wrapper, { ...login, password: 'elevenchars', password_again: 'elevenchars' })
    expect(router.currentRoute.value.path).toBe('/setup')
    expect(wrapper.get('#password-error').text()).toBe('Use at least 12 characters.')
    expect(wrapper.get('[name="password"]').attributes('aria-describedby')).toContain(
      'password-error',
    )
    expect(wrapper.get('[name="password"]').attributes('aria-invalid')).toBe('true')
  })

  it('refuses mismatched passwords with an error on the second', async () => {
    const { wrapper } = await toLogin()
    await submit(wrapper, {
      ...login,
      password: 'correct horse battery',
      password_again: 'correct horse batter',
    })
    expect(wrapper.get('#password_again-error').text()).toBe("The passwords don't match.")
    expect(wrapper.find('#password-error').exists()).toBe(false)
  })

  it('goes back to the currency when the server refuses it', async () => {
    server.refuse = { currency: 'Choose a currency from the list.' }
    const { wrapper } = await toLogin()
    await submit(wrapper, {
      ...login,
      password: 'correct horse battery',
      password_again: 'correct horse battery',
    })
    expect(wrapper.get('#currency-error').text()).toBe('Choose a currency from the list.')
  })

  it("keeps the login's errors after a trip back to the currency", async () => {
    server.refuse = { currency: 'Choose a currency from the list.' }
    const { wrapper } = await toLogin()
    await submit(wrapper, { ...login, password: 'elevenchars', password_again: 'elevenchars' })
    expect(wrapper.find('#currency-error').exists()).toBe(true)
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.find('#currency-error').exists()).toBe(false)
    expect(wrapper.get('#password-error').text()).toBe('Use at least 12 characters.')
  })

  it('asks for the code again if the app restarted with a new one', async () => {
    const { wrapper } = await toLogin()
    server.code = 'JKLM-NPQR'
    await submit(wrapper, {
      ...login,
      password: 'correct horse battery',
      password_again: 'correct horse battery',
    })
    expect(wrapper.get('#code-error').text()).toContain("That isn't the setup code.")
  })

  it("says so if the server can't be reached at the end", async () => {
    const { router, wrapper } = await toLogin()
    serverDown()
    await submit(wrapper, {
      ...login,
      password: 'correct horse battery',
      password_again: 'correct horse battery',
    })
    expect(router.currentRoute.value.path).toBe('/setup')
    expect(wrapper.get('form [role="alert"]').text()).toBe("Can't reach the server.")
  })

  it('finds IANA timezones by any part of their name', async () => {
    const { wrapper } = await open('/setup')
    await submit(wrapper, { code: SETUP_CODE })
    expect(await optionsFor(wrapper, 'timezone', 'chicago')).toEqual(['America/Chicago'])
  })
})

describe('before setup, with the server briefly unreachable', () => {
  it('still sends every address to the wizard once the server answers', async () => {
    server.setupDone = false
    serverDown()
    const { router } = await open('/accounts')
    serverUp()
    await router.push('/settings')
    expect(router.currentRoute.value.path).toBe('/setup')
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

describe('logging in', () => {
  beforeEach(() => {
    server.loggedIn = false
  })

  it('sends a logged-out visit to the log-in page, and back there afterwards', async () => {
    const { router, wrapper } = await open('/accounts')
    expect(router.currentRoute.value.path).toBe('/login')
    expect(router.currentRoute.value.query).toEqual({ next: '/accounts' })
    expect(wrapper.get('h1').text()).toBe('Log in')
    expect(wrapper.find('nav').exists()).toBe(false)
    expect(wrapper.get<HTMLInputElement>('[name="password"]').element.type).toBe('password')

    await submit(wrapper, { username: 'Sam', password: PASSWORD })
    expect(server.loggedIn).toBe(true)
    expect(router.currentRoute.value.path).toBe('/accounts')
    expect(wrapper.get('h1').text()).toBe('Accounts')
  })

  it('says the details are wrong, but not which', async () => {
    const { router, wrapper } = await open('/accounts')
    await submit(wrapper, { username: 'sam', password: 'correct horse batter' })
    expect(router.currentRoute.value.path).toBe('/login')
    expect(wrapper.get('form [role="alert"]').text()).toBe('Wrong username or password.')
    expect(wrapper.get<HTMLInputElement>('[name="password"]').element.value).toBe('')
  })

  it('says how long to wait once the throttle pauses the username', async () => {
    server.failures = 5
    const { router, wrapper } = await open('/login')
    await submit(wrapper, { username: 'sam', password: PASSWORD })
    expect(router.currentRoute.value.path).toBe('/login')
    expect(wrapper.get('form [role="alert"]').text()).toBe(
      'Too many attempts. Try again in 15 minutes.',
    )
  })

  it('lands on Transactions when there was nowhere before, or somewhere outside the app', async () => {
    for (const path of [
      '/login',
      '/login?next=//elsewhere.example.test/x',
      '/login?next=https://elsewhere.example.test',
    ]) {
      server.loggedIn = false
      const { router, wrapper } = await open(path)
      await submit(wrapper, { username: 'sam', password: PASSWORD })
      expect(router.currentRoute.value.path, path).toBe('/transactions')
    }
  })

  it('sends the app to log in when a call finds the session gone, and back afterwards', async () => {
    server.loggedIn = true
    const { router, wrapper } = await open('/categories')
    server.loggedIn = false
    await call(client.GET('/api/v1/household')).catch(() => undefined)
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/login')
    expect(router.currentRoute.value.query).toEqual({ next: '/categories' })

    await submit(wrapper, { username: 'sam', password: PASSWORD })
    expect(router.currentRoute.value.path).toBe('/categories')
  })

  it("shows the screen anyway, under the banner, if the server can't be asked", async () => {
    serverDown()
    const { router, wrapper } = await open('/accounts')
    expect(router.currentRoute.value.path).toBe('/accounts')
    expect(wrapper.get('[role="alert"]').text()).toContain("Can't reach the server")
  })
  it('shows the screen anyway if the server goes away between the two questions', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn((request: Request) =>
        new URL(request.url).pathname === '/api/v1/session'
          ? Promise.reject(new TypeError('Failed to fetch'))
          : answer(request),
      ),
    )
    const { router } = await open('/accounts')
    expect(router.currentRoute.value.path).toBe('/accounts')
  })
})

describe('logging out', () => {
  it('is under More, and ends on the log-in page', async () => {
    const { router, wrapper } = await open('/more')
    const button = wrapper
      .get('main')
      .findAll('button')
      .find((b) => b.text() === 'Log out')
    await button?.trigger('click')
    await flushPromises()
    expect(server.loggedIn).toBe(false)
    expect(router.currentRoute.value.path).toBe('/login')
    expect(router.currentRoute.value.query).toEqual({})
  })

  it('is at the foot of the desktop sidebar too', async () => {
    const { router, wrapper } = await open('/transactions')
    await wrapper.get('nav[aria-label="Sidebar"] button').trigger('click')
    await flushPromises()
    expect(server.loggedIn).toBe(false)
    expect(router.currentRoute.value.path).toBe('/login')
  })

  it('asks for a login again on the next visit', async () => {
    const { router, wrapper } = await open('/more')
    await wrapper.get('main button').trigger('click')
    await flushPromises()
    await router.push('/accounts')
    expect(router.currentRoute.value.path).toBe('/login')
    expect(router.currentRoute.value.query).toEqual({ next: '/accounts' })
  })
})
