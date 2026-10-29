// The one place that calls the server. Request and response types come from generated/schema.ts,
// which `just gen-api` writes from the Rust API (docs/adr/0007-generated-api-contract.md).
import createClient from 'openapi-fetch'
import type { components, paths } from './generated/schema'

/** A call that didn't succeed: the server's error envelope, or why there's no envelope. */
export class ApiError extends Error {
  /** The HTTP status, or 0 when no answer came. */
  readonly status: number
  /** The envelope's `code`; `unreachable` or `unexpected_response` when there's no envelope. */
  readonly code: string
  /** Validation errors only: each field that's wrong and why. */
  readonly fields: Readonly<Record<string, string>>

  constructor(status: number, code: string, message: string, fields: Record<string, string> = {}) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.fields = fields
  }
}

export const client = createClient<paths>({
  baseUrl: globalThis.location.origin,
  // Looked up on every call rather than captured here, so tests can stub it.
  fetch: (request) => globalThis.fetch(request),
})

let loginRequired: () => void = () => undefined

/**
 * What to do when a call finds there's no live session (`login_required`): the router sends the app
 * to the log-in page. The call still throws.
 */
export function onLoginRequired(handler: () => void): void {
  loginRequired = handler
}

type Answer<T> =
  | { data: T; error?: never; response: Response }
  | { data?: never; error: unknown; response: Response }

/**
 * The data of a successful call; anything else throws an ApiError. A `login_required` answer also
 * runs the onLoginRequired handler, unless `quiet` says the caller deals with it.
 */
export async function call<T>(
  request: Promise<Answer<T>>,
  { quiet = false }: { quiet?: boolean } = {},
): Promise<T> {
  let answer: Answer<T>
  try {
    answer = await request
  } catch {
    throw new ApiError(0, 'unreachable', "Can't reach the server.")
  }
  if (answer.response.ok) return answer.data as T
  const { status } = answer.response
  if (!isEnvelope(answer.error)) {
    throw new ApiError(
      status,
      'unexpected_response',
      "The server's answer wasn't one the app knows.",
    )
  }
  const { code, message, fields } = answer.error.error
  if (code === 'login_required' && !quiet) loginRequired()
  throw new ApiError(status, code, message, fields)
}

function isEnvelope(body: unknown): body is components['schemas']['ErrorEnvelope'] {
  if (typeof body !== 'object' || body === null || !('error' in body)) return false
  const { error } = body
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof error.code === 'string' &&
    'message' in error &&
    typeof error.message === 'string'
  )
}
