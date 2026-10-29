import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, call, client, onLoginRequired } from './client'
import { getSession, logOut } from './session'

const loggedOut = '{"error":{"code":"login_required","message":"Log in to continue."}}'

function answer(status: number, body: string | null) {
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

afterEach(() => {
  vi.unstubAllGlobals()
  onLoginRequired(() => undefined)
})

describe('a call that finds no session', () => {
  it('runs the login-required handler and still throws', async () => {
    const handler = vi.fn()
    onLoginRequired(handler)
    answer(401, loggedOut)
    await expect(call(client.GET('/api/v1/household'))).rejects.toBeInstanceOf(ApiError)
    expect(handler).toHaveBeenCalledOnce()
  })

  it("leaves the handler alone when it's quiet, or for any other error", async () => {
    const handler = vi.fn()
    onLoginRequired(handler)
    answer(401, loggedOut)
    await expect(call(client.GET('/api/v1/household'), { quiet: true })).rejects.toThrow(
      'Log in to continue.',
    )
    answer(401, '{"error":{"code":"login_failed","message":"Wrong username or password."}}')
    await expect(call(client.GET('/api/v1/household'))).rejects.toThrow()
    expect(handler).not.toHaveBeenCalled()
  })
})

describe('getSession', () => {
  it('names the login, or null when nobody is logged in', async () => {
    answer(200, '{"username":"sam","display_name":"Sam"}')
    expect(await getSession()).toEqual({ username: 'sam', display_name: 'Sam' })
    answer(401, loggedOut)
    expect(await getSession()).toBeNull()
  })

  it('throws anything else', async () => {
    answer(403, '{"error":{"code":"setup_required","message":"Finish setting up the app first."}}')
    await expect(getSession()).rejects.toThrow('Finish setting up the app first.')
  })
})

describe('logOut', () => {
  it('sends a JSON DELETE', async () => {
    answer(204, null)
    await logOut()
    expect(sent().method).toBe('DELETE')
    expect(sent().headers.get('Content-Type')).toBe('application/json')
  })

  it('counts already logged out as done, and throws anything else', async () => {
    answer(401, loggedOut)
    await expect(logOut()).resolves.toBeUndefined()
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
    )
    await expect(logOut()).rejects.toThrow("Can't reach the server.")
  })
})
