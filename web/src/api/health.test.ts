import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './client'
import { getHealth } from './health'

function answer(status: number, body: string, contentType = 'application/json') {
  vi.stubGlobal(
    'fetch',
    vi.fn(() =>
      Promise.resolve(new Response(body, { status, headers: { 'Content-Type': contentType } })),
    ),
  )
}

async function failure(promise: Promise<unknown>): Promise<ApiError> {
  const error: unknown = await promise.then(
    () => undefined,
    (reason: unknown) => reason,
  )
  if (!(error instanceof ApiError)) throw new Error(`expected an ApiError, got ${String(error)}`)
  return error
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getHealth', () => {
  it('asks the server for its health', async () => {
    answer(200, '{"status":"ok"}')
    expect(await getHealth()).toEqual({ status: 'ok' })
    const [request] = vi.mocked(fetch).mock.calls[0] ?? []
    expect(request instanceof Request && new URL(request.url).pathname).toBe('/api/v1/health')
  })

  it("turns the server's error envelope into an ApiError", async () => {
    answer(
      503,
      '{"error":{"code":"database_unavailable","message":"The database isn\'t answering."}}',
    )
    const error = await failure(getHealth())
    expect(error).toMatchObject({
      status: 503,
      code: 'database_unavailable',
      message: "The database isn't answering.",
      fields: {},
    })
  })

  it('keeps the fields of a validation error', async () => {
    answer(
      422,
      '{"error":{"code":"validation_failed","message":"Check the form.","fields":{"name":"Give it a name."}}}',
    )
    const error = await failure(getHealth())
    expect(error.fields).toEqual({ name: 'Give it a name.' })
  })

  it('says the server is unreachable when no answer comes', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
    )
    const error = await failure(getHealth())
    expect(error).toMatchObject({ status: 0, code: 'unreachable' })
  })

  it('says so when an error comes without the envelope, as from a proxy', async () => {
    answer(502, '<html>Bad Gateway</html>', 'text/html')
    const error = await failure(getHealth())
    expect(error).toMatchObject({ status: 502, code: 'unexpected_response' })
  })
})
