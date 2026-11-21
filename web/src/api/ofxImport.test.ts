import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './client'
import { commitOfxImport, previewOfxImport, readOfxFile } from './ofxImport'

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

async function sentBody(): Promise<{ path: string; body: unknown }> {
  const [request] = vi.mocked(fetch).mock.calls[0] ?? []
  if (!(request instanceof Request)) throw new Error('nothing was sent')
  return { path: new URL(request.url).pathname, body: await request.json() }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('readOfxFile', () => {
  it('sends the text and resolves to the account it is for', async () => {
    const read = {
      last_four: '0042',
      account_id: 3,
      transactions: 2,
      first_date: '2026-09-01',
      last_date: '2026-09-02',
    }
    answer(200, JSON.stringify(read))
    await expect(readOfxFile('<OFX>')).resolves.toEqual(read)
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/ofx/read',
      body: { content: '<OFX>' },
    })
  })

  it('throws the code of a file that is not OFX', async () => {
    answer(422, JSON.stringify({ error: { code: 'not_ofx', message: 'Not OFX.' } }))
    await expect(readOfxFile('x')).rejects.toBeInstanceOf(ApiError)
  })
})

describe('previewOfxImport and commitOfxImport', () => {
  it('send the account, the text and the rows kept', async () => {
    answer(200, JSON.stringify({ new: 1 }))
    await previewOfxImport(3, '<OFX>')
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/ofx/preview',
      body: { account_id: 3, content: '<OFX>' },
    })
    vi.unstubAllGlobals()
    answer(201, JSON.stringify({ batch_id: 1, added: 1, matched: 0, skipped: 0 }))
    await commitOfxImport({ accountId: 3, fileName: 'a.qfx', content: '<OFX>', keep: [2] })
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/ofx/commit',
      body: { account_id: 3, file_name: 'a.qfx', content: '<OFX>', keep: [2] },
    })
  })
})
