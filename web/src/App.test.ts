import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory } from 'vue-router'
import App from './App.vue'
import type { Account } from './api/accounts'
import type { Transaction } from './api/transactions'
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
  /** The login's password: the one setup chose, until it's changed or reset. */
  password: string
  /** The login's saved locale and week start. */
  locale: string | null
  weekStart: number | null
  /** The reset link's token that still works, if any. */
  resetToken?: string | undefined
  /** The category groups, as `GET /api/v1/categories` lists them. */
  groups: { id: number; name: string; kind: string; hidden: boolean; categories: unknown[] }[]
  /** The accounts, as `GET /api/v1/accounts` lists them. */
  accounts: Account[]
  /** The transactions in full, newest first; `GET /api/v1/transactions` pages them by 2. */
  transactions: Transaction[]
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
  if (path === '/api/v1/sessions' && request.method === 'DELETE') {
    server.loggedIn = false
    return new Response(null, { status: 204 })
  }
  if (path.startsWith('/api/v1/profile')) return profile(request, path)
  if (path.startsWith('/api/v1/password-reset')) return reset(request, path)
  if (path.startsWith('/api/v1/categor')) return categories(request, path)
  if (path.startsWith('/api/v1/accounts') && !server.loggedIn) {
    return error(401, 'login_required', 'Log in to continue.')
  }
  if (path === '/api/v1/accounts') return Response.json({ accounts: server.accounts })
  if (path.startsWith('/api/v1/accounts/')) {
    const found = server.accounts.find((a) => `/api/v1/accounts/${String(a.id)}` === path)
    return found ? Response.json(found) : error(404, 'not_found', "There's no such account.")
  }
  if (path.startsWith('/api/v1/transactions')) return transactions(request, path)
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

function transactions(request: Request, path: string): Response {
  if (!server.loggedIn) return error(401, 'login_required', 'Log in to continue.')
  if (path === '/api/v1/transactions') {
    // The cursor is the index to start at.
    const start = Number(new URL(request.url).searchParams.get('after') ?? '0')
    const page = server.transactions.slice(start, start + 2)
    const next = start + 2 < server.transactions.length ? String(start + 2) : null
    return Response.json({
      transactions: page.map((t) => ({
        id: t.id,
        date: t.date,
        amount_minor: t.amount_minor,
        status: t.status,
        payee: t.payee,
        raw_description: t.raw_description,
        account_id: t.account_id,
        account: t.account,
        category_id: t.category_id,
        category: t.category,
        split: t.split,
        tags: t.tags,
      })),
      next_cursor: next,
    })
  }
  const found = server.transactions.find((t) => `/api/v1/transactions/${String(t.id)}` === path)
  return found ? Response.json(found) : error(404, 'not_found', "There's no such transaction.")
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
  if (username?.trim().toLowerCase() !== 'sam' || password !== server.password) {
    server.failures += 1
    return error(401, 'login_failed', 'Wrong username or password.')
  }
  server.failures = 0
  server.loggedIn = true
  return new Response(null, { status: 204 })
}

function invalid(fields: Record<string, string>) {
  return Response.json(
    { error: { code: 'validation_failed', message: 'Some details need another look.', fields } },
    { status: 422 },
  )
}

function passwordProblems(password = '', again = ''): Record<string, string> {
  if (password.length < 12) return { password: 'Use at least 12 characters.' }
  if (password !== again) return { password_again: "The passwords don't match." }
  return {}
}

async function profile(request: Request, path: string): Promise<Response> {
  if (!server.loggedIn) return error(401, 'login_required', 'Log in to continue.')
  if (path === '/api/v1/profile/password') {
    const body = (await request.json()) as Record<string, string>
    if (body.current_password !== server.password) {
      return invalid({ current_password: "That isn't your current password." })
    }
    const problems = passwordProblems(body.password, body.password_again)
    if (Object.keys(problems).length > 0) return invalid(problems)
    server.password = body.password ?? ''
    return new Response(null, { status: 204 })
  }
  if (request.method === 'PUT') {
    const body = (await request.json()) as {
      display_locale: string | null
      week_start: number | null
    }
    if (body.display_locale === 'de-AT') {
      return invalid({ display_locale: 'Choose a locale from the list.' })
    }
    server.locale = body.display_locale
    server.weekStart = body.week_start
  }
  return Response.json({
    username: 'sam',
    display_name: 'Sam',
    display_locale: server.locale,
    week_start: server.weekStart,
  })
}

const dead = () =>
  error(
    410,
    'reset_link_dead',
    'This reset link has expired or was already used. Ask for a new one.',
  )

async function reset(request: Request, path: string): Promise<Response> {
  const body = (await request.json()) as Record<string, string>
  if (server.resetToken === undefined || body.token !== server.resetToken) return dead()
  if (path === '/api/v1/password-reset/check') return new Response(null, { status: 204 })
  const problems = passwordProblems(body.password, body.password_again)
  if (Object.keys(problems).length > 0) return invalid(problems)
  server.password = body.password ?? ''
  server.resetToken = undefined
  server.loggedIn = true
  return new Response(null, { status: 204 })
}

async function categories(request: Request, path: string): Promise<Response> {
  if (path === '/api/v1/categories/starter') {
    server.groups = [
      {
        id: 1,
        name: 'Food',
        kind: 'expense',
        hidden: false,
        categories: [{ id: 11, name: 'Groceries', hidden: false }],
      },
      { id: 2, name: 'Pets', kind: 'expense', hidden: true, categories: [] },
    ]
    return new Response(null, { status: 204 })
  }
  if (path === '/api/v1/category-groups' && request.method === 'POST') {
    const { name, kind } = (await request.json()) as { name: string; kind: string }
    if (server.groups.some((group) => group.name === name)) {
      return invalid({ name: `There's already a group called ${name}.` })
    }
    server.groups.push({ id: 3, name, kind, hidden: false, categories: [] })
    return Response.json({ id: 3 }, { status: 201 })
  }
  return Response.json({ groups: server.groups })
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
  server = {
    setupDone: true,
    loggedIn: true,
    failures: 0,
    code: SETUP_CODE,
    password: PASSWORD,
    locale: null,
    weekStart: null,
    groups: [],
    accounts: [],
    transactions: [],
  }
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
    // A screen that asks the server nothing itself: one that does would find the login gone.
    const { router } = await open('/more')
    expect(router.currentRoute.value.path).toBe('/more')
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

describe('Settings', () => {
  it('lists Profile and Categories, and opens Profile', async () => {
    const { router, wrapper } = await open('/settings')
    const links = wrapper.get('main').findAll('a')
    expect(links.map((a) => [a.text(), a.attributes('href')])).toEqual([
      ['Profile', '/settings/profile'],
      ['Categories', '/categories'],
    ])
    await links[0]?.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/settings/profile')
    expect(wrapper.get('h1').text()).toBe('Profile')
  })
})

describe('Settings › Profile', () => {
  const example = (wrapper: Wrapper) => wrapper.get('[data-testid="format-example"]').text()

  it('shows the saved name, and follows the browser until told otherwise', async () => {
    const { wrapper } = await open('/settings/profile')
    expect(wrapper.get<HTMLInputElement>('[name="display_name"]').element.value).toBe('Sam')
    expect(wrapper.get<HTMLInputElement>('#display_locale').element.value).toBe(
      'Browser default (English (United States) · en-US)',
    )
    expect(wrapper.get<HTMLSelectElement>('#week_start').element.value).toBe('')
    expect(wrapper.get('#week_start option').text()).toBe('Sunday, from the locale')
    expect(example(wrapper)).toBe(
      'Amounts look like -€1,234.56 and dates like Mar 1, 2026; weeks start on Sunday.',
    )
  })

  it('changes number and date formats at once when the locale changes to de-DE', async () => {
    const { wrapper } = await open('/settings/profile')
    await choose(wrapper, 'display_locale', 'german germany', 'German (Germany) · de-DE')
    expect(wrapper.get('#week_start option').text()).toBe('Monday, from the locale')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(server.locale).toBe('de-DE')
    expect(wrapper.get('[role="status"]').text()).toBe('Saved.')
    expect(example(wrapper)).toBe(
      'Amounts look like -1.234,56\u00a0€ and dates like 01.03.2026; weeks start on Monday.',
    )
  })

  it("starts weeks on the chosen day, and goes back to the browser's ways", async () => {
    server.locale = 'de-DE'
    const { wrapper } = await open('/settings/profile')
    await wrapper.get('#week_start').setValue('6')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(server.weekStart).toBe(6)
    expect(example(wrapper)).toContain('weeks start on Saturday')

    await choose(
      wrapper,
      'display_locale',
      'browser',
      'Browser default (English (United States) · en-US)',
    )
    await wrapper.get('#week_start').setValue('')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect([server.locale, server.weekStart]).toEqual([null, null])
    expect(example(wrapper)).toContain('-€1,234.56')
  })

  it('shows what the server refuses on its field', async () => {
    const { wrapper } = await open('/settings/profile')
    await choose(wrapper, 'display_locale', 'austria', 'German (Austria) · de-AT')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('#display_locale-error').text()).toBe('Choose a locale from the list.')
  })

  it("says so if the server can't be reached", async () => {
    const { wrapper } = await open('/settings/profile')
    serverDown()
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('form [role="alert"]').text()).toBe("Can't reach the server.")
  })

  async function changePassword(wrapper: Wrapper, values: Record<string, string>) {
    const form = wrapper.findAll('form')[1]
    if (form === undefined) throw new Error('no password form')
    for (const [name, value] of Object.entries(values)) {
      await form.get(`[name="${name}"]`).setValue(value)
    }
    await form.trigger('submit')
    await flushPromises()
    return form
  }

  it('changes the password, given the current one', async () => {
    const { wrapper } = await open('/settings/profile')
    const fresh = 'a whole new passphrase'
    let form = await changePassword(wrapper, {
      current_password: 'not my password',
      password: fresh,
      password_again: fresh,
    })
    expect(form.get('#current_password-error').text()).toBe("That isn't your current password.")
    form = await changePassword(wrapper, {
      current_password: PASSWORD,
      password: fresh,
      password_again: `${fresh}!`,
    })
    expect(form.get('#password_again-error').text()).toBe("The passwords don't match.")

    form = await changePassword(wrapper, {
      current_password: PASSWORD,
      password: fresh,
      password_again: fresh,
    })
    expect(server.password).toBe(fresh)
    expect(form.get('[role="status"]').text()).toBe('Password changed.')
    expect(form.get<HTMLInputElement>('[name="current_password"]').element.value).toBe('')
  })

  it("says so if the server can't be reached to change the password", async () => {
    const { wrapper } = await open('/settings/profile')
    serverDown()
    const form = await changePassword(wrapper, {
      current_password: PASSWORD,
      password: 'a whole new passphrase',
      password_again: 'a whole new passphrase',
    })
    expect(form.get('[role="alert"]').text()).toBe("Can't reach the server.")
  })

  function everywhere(wrapper: Wrapper) {
    const button = wrapper.findAll('button').find((b) => b.text() === 'Log out everywhere')
    if (button === undefined) throw new Error('no Log out everywhere')
    return button
  }

  it('logs out everywhere and ends on the log-in page', async () => {
    const { router, wrapper } = await open('/settings/profile')
    await everywhere(wrapper).trigger('click')
    await flushPromises()
    expect(server.loggedIn).toBe(false)
    expect(router.currentRoute.value.path).toBe('/login')
    expect(router.currentRoute.value.query).toEqual({})
  })

  it("stays put and says so if the server can't be reached to log out everywhere", async () => {
    const { router, wrapper } = await open('/settings/profile')
    serverDown()
    await everywhere(wrapper).trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/settings/profile')
    expect(wrapper.text()).toContain("Can't reach the server.")
  })
})

describe('the reset link', () => {
  const TOKEN = 'ab'.repeat(32)

  beforeEach(() => {
    server.loggedIn = false
    server.resetToken = TOKEN
  })

  it('opens the set-new-password page, takes the token out of the address, and logs in', async () => {
    const { router, wrapper } = await open(`/reset-password#${TOKEN}`)
    expect(wrapper.get('h1').text()).toBe('Set a new password')
    expect(wrapper.find('nav').exists()).toBe(false)
    expect(router.currentRoute.value.fullPath).toBe('/reset-password')

    await submit(wrapper, { password: 'elevenchars', password_again: 'elevenchars' })
    expect(wrapper.get('#password-error').text()).toBe('Use at least 12 characters.')

    const fresh = 'a whole new passphrase'
    await submit(wrapper, { password: fresh, password_again: fresh })
    expect(server.password).toBe(fresh)
    expect(router.currentRoute.value.path).toBe('/transactions')
    expect(wrapper.get('h1').text()).toBe('Transactions')
  })

  it('says a used or expired link is dead, and offers to log in', async () => {
    server.resetToken = undefined
    const { wrapper } = await open(`/reset-password#${TOKEN}`)
    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.get('[role="alert"]').text()).toBe(
      'This reset link has expired or was already used. Ask for a new one.',
    )
    expect(wrapper.get('main a').attributes('href')).toBe('/login')
  })

  it('says so when the link dies while the page is open', async () => {
    const { wrapper } = await open(`/reset-password#${TOKEN}`)
    server.resetToken = undefined
    const fresh = 'a whole new passphrase'
    await submit(wrapper, { password: fresh, password_again: fresh })
    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.get('[role="alert"]').text()).toContain('This reset link has expired')
  })

  it('says so when the address has no token', async () => {
    const { wrapper } = await open('/reset-password')
    expect(wrapper.get('[role="alert"]').text()).toBe(
      'This address has no reset link in it. Copy the whole link again.',
    )
  })

  it("still offers the form if the server can't be asked about the link", async () => {
    serverDown()
    const { wrapper } = await open(`/reset-password#${TOKEN}`)
    expect(wrapper.find('form').exists()).toBe(true)
    expect(wrapper.get('form [role="alert"]').text()).toBe("Can't reach the server.")
  })
})

describe('Settings › Categories', () => {
  const buttons = (wrapper: Awaited<ReturnType<typeof open>>['wrapper']) =>
    wrapper
      .get('main')
      .findAll('button')
      .map((button) => button.text())

  it('offers the starter set or an empty start while there are none, and writes the starters', async () => {
    const { wrapper } = await open('/categories')
    expect(buttons(wrapper)).toEqual(['Use starter categories', 'Start empty'])
    const starter = wrapper.findAll('button').find((b) => b.text() === 'Use starter categories')
    await starter?.trigger('click')
    await flushPromises()
    expect(wrapper.findAll('main h2').map((h) => h.text())).toEqual(['Food', 'Hidden'])
    expect(wrapper.get('main').text()).toContain('Groceries')
    expect(wrapper.get('main').text()).toMatch(/Pets\s+· group, 0 categories/)
  })

  it('starts empty with a way to add a group, and names a taken name', async () => {
    const { wrapper } = await open('/categories')
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Start empty')
      ?.trigger('click')
    await wrapper
      .findAll('button')
      .find((b) => b.text() === '+ Group')
      ?.trigger('click')
    await wrapper.get('#new-group-name').setValue('Income')
    await wrapper.get('select[aria-label="Kind"]').setValue('income')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.findAll('main h2').map((h) => h.text())).toEqual(['Income'])
    expect(wrapper.get('main').text()).toContain('Income')

    await wrapper
      .findAll('button')
      .find((b) => b.text() === '+ Group')
      ?.trigger('click')
    await wrapper.get('#new-group-name').setValue('Income')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toBe("There's already a group called Income.")
  })
})

describe('Accounts', () => {
  const fake = (id: number, fields: Partial<Account>): Account => ({
    id,
    name: `Account ${String(id)}`,
    institution: null,
    type: 'checking',
    role: 'budgeted',
    closed: false,
    balances_only: false,
    balance_minor: 0,
    app_balance_minor: 0,
    opening_balance_minor: 0,
    opening_date: '2025-12-31',
    rows: 0,
    last_transaction_date: null,
    ...fields,
  })

  beforeEach(() => {
    server.accounts = [
      fake(1, {
        name: 'Everyday checking',
        institution: 'Fake Bank',
        balance_minor: 2_145_801,
        app_balance_minor: 2_145_801,
        opening_balance_minor: 150_000,
        rows: 42,
        last_transaction_date: '2026-10-01',
      }),
      fake(2, { name: 'Old checking', closed: true }),
      fake(3, {
        name: 'Brokerage',
        type: 'investment',
        role: 'tracking',
        balances_only: true,
        balance_minor: 1_000_000,
        app_balance_minor: null,
        opening_balance_minor: null,
        opening_date: null,
      }),
    ]
  })

  it('offers the import while there are none', async () => {
    server.accounts = []
    const { wrapper } = await open('/accounts')
    expect(wrapper.get('main').text()).toContain('No accounts yet.')
    const link = wrapper.get('main').get('a')
    expect([link.text(), link.attributes('href')]).toEqual(['Import from Actual', '/import'])
  })

  it('groups them on phones and tables them on desktops, in the household currency', async () => {
    const { wrapper } = await open('/accounts')
    const groups = wrapper.findAll('main section')
    expect(groups.map((g) => [g.get('h2').text(), g.findAll('li').map((li) => li.text())])).toEqual(
      [
        ['Budgeted', ['Everyday checkingChecking€21,458.01']],
        ['Tracking', ['BrokerageInvestment€10,000.00']],
        ['Closed', ['Old checkingChecking€0.00']],
      ],
    )
    const rows = wrapper.get('table[aria-label="Accounts"]').findAll('tbody tr')
    expect(rows.map((row) => row.findAll('th, td').map((cell) => cell.text()))).toEqual([
      ['Everyday checking', 'Fake Bank', 'Checking', 'Budgeted', '€21,458.01', 'Oct 1, 2026'],
      ['Brokerage', '—', 'Investment', 'Tracking', '€10,000.00', '—'],
      ['Old checkingClosed', '—', 'Checking', 'Budgeted', '€0.00', '—'],
    ])
  })

  it('opens an account with its balances and a link to its transactions', async () => {
    const { router, wrapper } = await open('/accounts')
    await wrapper.get('main li a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/accounts/1')
    expect(wrapper.get('h2').text()).toBe('Everyday checking')
    expect(wrapper.get('main section p').text()).toBe('Checking · Budgeted · Fake Bank')
    const figures = wrapper
      .get('dl')
      .findAll('dt, dd')
      .map((cell) => cell.text())
    expect(figures).toEqual([
      'Balance',
      '€21,458.01',
      "App's balance",
      '€21,458.01',
      'Opening balance',
      '€1,500.00 on Dec 31, 2025',
      'Transactions',
      '42',
    ])
    const link = wrapper.findAll('main a').find((a) => a.text() === 'See transactions')
    expect(link?.attributes('href')).toBe('/transactions?account=1')
  })

  it('shows a balances-only account with its balance and no transactions', async () => {
    const { wrapper } = await open('/accounts/3')
    expect(
      wrapper
        .get('dl')
        .findAll('dt, dd')
        .map((cell) => cell.text()),
    ).toEqual(['Balance', '€10,000.00'])
    expect(wrapper.get('main').text()).toContain('Balances only')
    expect(wrapper.findAll('main a').map((a) => a.text())).toEqual(['← All accounts'])
  })

  it('says so for an account that is not there', async () => {
    const { wrapper } = await open('/accounts/9')
    expect(wrapper.get('[role="alert"]').text()).toBe("There's no such account.")
  })
})

describe('Transactions', () => {
  const fake = (id: number, fields: Partial<Transaction>): Transaction => ({
    id,
    date: '2026-01-05',
    amount_minor: -1250,
    status: 'posted',
    payee: 'Corner Market',
    raw_description: `CORNER MKT ${String(id)}`,
    account_id: 1,
    account: 'Checking',
    category_id: null,
    category: null,
    split: false,
    tags: [],
    memo: null,
    splits: [],
    source: 'import',
    categorization_reason: null,
    import_batch: { id: 1, file_name: 'export.zip', imported_at: '2026-02-01T10:00:00Z' },
    ...fields,
  })

  beforeEach(() => {
    server.transactions = [
      fake(5, {
        category_id: 1,
        category: 'Groceries',
        tags: ['trip'],
        categorization_reason: 'From Actual: Groceries',
      }),
      fake(4, { payee: null, raw_description: 'REFUND 0042', amount_minor: 4000 }),
      fake(3, {
        date: '2026-01-03',
        amount_minor: -12_000,
        account: 'Card',
        memo: 'Big shop\nSplit in Actual: Groceries -80.00; Dining out -40.00',
      }),
      fake(2, {
        date: '2026-01-02',
        status: 'pending',
        split: true,
        source: 'provider',
        import_batch: null,
        splits: [
          { category_id: 1, category: 'Groceries', amount_minor: -800, memo: 'food' },
          { category_id: 2, category: 'Dining out', amount_minor: -450, memo: null },
        ],
      }),
    ]
  })

  it('lists them by day on phones, with money in marked plus', async () => {
    const { wrapper } = await open('/transactions')
    const days = wrapper.findAll('main section')
    expect(days.map((d) => [d.get('h2').text(), d.findAll('li').map((li) => li.text())])).toEqual([
      [
        'Jan 5, 2026',
        ['Corner Market Groceries-€12.50Checking', 'REFUND 0042 Uncategorized+€40.00Checking'],
      ],
    ])
  })

  it('tables them on desktops, and loads more at the end', async () => {
    const { wrapper } = await open('/transactions')
    const table = wrapper.get('table[aria-label="Transactions"]')
    expect(table.findAll('th').map((th) => th.text())).toEqual([
      'Date',
      'Payee',
      'Category',
      'Account',
      'Amount',
      'Tags',
    ])
    const cells = () =>
      table.findAll('tbody tr').map((row) => row.findAll('td').map((cell) => cell.text()))
    expect(cells()).toEqual([
      ['Jan 5, 2026', 'Corner Market', 'Groceries', 'Checking', '-€12.50', 'trip'],
      ['Jan 5, 2026', 'REFUND 0042', 'Uncategorized', 'Checking', '+€40.00', ''],
    ])
    const more = wrapper.findAll('main button').find((b) => b.text() === 'Load more')
    await more?.trigger('click')
    await flushPromises()
    expect(cells().map((row) => [row[1], row[2], row[4]])).toEqual([
      ['Corner Market', 'Groceries', '-€12.50'],
      ['REFUND 0042', 'Uncategorized', '+€40.00'],
      ['Corner Market', 'Uncategorized', '-€120.00'],
      ['Corner Market', 'Split', '-€12.50'],
    ])
    expect(wrapper.findAll('main button').map((b) => b.text())).not.toContain('Load more')
    expect(wrapper.findAll('main section').map((d) => d.get('h2').text())).toEqual([
      'Jan 5, 2026',
      'Jan 3, 2026',
      'Jan 2, 2026',
    ])
  })

  it('opens one in full, and closes it again', async () => {
    const { router, wrapper } = await open('/transactions')
    await wrapper.get('main li a').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/transactions/5')
    const detail = wrapper.get('aside')
    expect(detail.get('h2').text()).toBe('Corner Market')
    expect(detail.text()).toContain('-€12.50')
    const pairs = detail.findAll('dt').map((dt, i) => [dt.text(), detail.findAll('dd')[i]?.text()])
    expect(pairs).toEqual([
      ['Date', 'Jan 5, 2026'],
      ['Account', 'Checking'],
      ['Payee', 'Corner Market'],
      ['Bank description', 'CORNER MKT 5'],
      ['Category', 'Groceries'],
      ['Memo', '—'],
      ['Tags', 'trip'],
      ['Status', 'Posted'],
      ['Source', 'Import'],
      ['Categorized', 'From Actual: Groceries'],
      ['Import', 'export.zip, Feb 1, 2026'],
    ])
    const row = wrapper.get('table tbody tr[aria-current="true"]')
    expect(row.text()).toContain('Corner Market')
    await detail.get('button').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/transactions')
    expect(wrapper.find('aside').exists()).toBe(false)
  })

  it('shows the split lines, the memo and where it came from', async () => {
    const { wrapper } = await open('/transactions/2')
    const detail = wrapper.get('aside')
    expect(detail.get('h2').text()).toBe('Corner Market')
    expect(
      detail
        .get('ul[aria-label="Split lines"]')
        .findAll('li')
        .map((li) => li.text()),
    ).toEqual(['Groceries · food-€8.00', 'Dining out -€4.50'])
    const text = detail.text()
    expect(text).toContain('Pending')
    expect(text).toContain('Bank')
    expect(text).not.toContain('export.zip')
  })

  it("shows a fallback split's Actual split in its memo, and a payee-less row's bank text", async () => {
    let { wrapper } = await open('/transactions/3')
    expect(wrapper.get('aside').text()).toContain(
      'Big shop\nSplit in Actual: Groceries -80.00; Dining out -40.00',
    )
    ;({ wrapper } = await open('/transactions/4'))
    expect(wrapper.get('aside h2').text()).toBe('REFUND 0042')
    expect(wrapper.get('aside').text()).toContain('+€40.00')
  })

  it('closes the open one on Escape', async () => {
    const { router } = await open('/transactions/5')
    globalThis.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/transactions')
  })

  it('says so for a transaction that is not there', async () => {
    const { wrapper } = await open('/transactions/9')
    expect(wrapper.get('aside [role="alert"]').text()).toBe("There's no such transaction.")
    expect(wrapper.get('aside h2').text()).toBe('Transaction')
  })
})
