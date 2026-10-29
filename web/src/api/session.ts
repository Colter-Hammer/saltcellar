import { ApiError, call, client } from './client'
import type { components } from './generated/schema'

export type Session = components['schemas']['Session']

function isLoggedOut(error: unknown): boolean {
  return error instanceof ApiError && error.code === 'login_required'
}

/** `GET /api/v1/session`: who is logged in, or null when nobody is. */
export async function getSession(): Promise<Session | null> {
  try {
    return await call(client.GET('/api/v1/session'), { quiet: true })
  } catch (error) {
    if (isLoggedOut(error)) return null
    throw error
  }
}

/**
 * `POST /api/v1/session`: logs in (the server sets the session cookie). Throws an ApiError for a
 * wrong username or password (`login_failed`), or while the throttle pauses it (`too_many_attempts`).
 */
export async function logIn(username: string, password: string): Promise<void> {
  await call(client.POST('/api/v1/session', { body: { username, password } }))
}

/** `DELETE /api/v1/session`: logs out. Already logged out counts as done. */
export async function logOut(): Promise<void> {
  try {
    // Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
    await call(
      client.DELETE('/api/v1/session', { headers: { 'Content-Type': 'application/json' } }),
      { quiet: true },
    )
  } catch (error) {
    if (!isLoggedOut(error)) throw error
  }
}
