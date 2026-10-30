import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './client'
import { commitActualImport, getActualImport, uploadActualExport } from './imports'

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

function sent(): Request {
  const [request] = vi.mocked(fetch).mock.calls[0] ?? []
  if (!(request instanceof Request)) throw new Error('nothing was sent')
  return request
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const upload = {
  id: 'abc',
  budget_name: 'Example Household',
  accounts: 5,
  transactions: 80,
  categories: 7,
  first_date: '2025-01-01',
  last_date: '2026-01-05',
}

describe('uploadActualExport', () => {
  it('sends the zip itself and resolves to what is in it', async () => {
    answer(201, JSON.stringify(upload))
    const file = new File([new Uint8Array([80, 75, 3, 4])], 'export.zip')
    expect(await uploadActualExport(file)).toEqual(upload)
    expect(sent().method).toBe('POST')
    expect(new URL(sent().url).pathname).toBe('/api/v1/imports/actual')
    expect(sent().headers.get('Content-Type')).toBe('application/zip')
    expect(new Uint8Array(await sent().arrayBuffer())).toEqual(new Uint8Array([80, 75, 3, 4]))
  })

  it('throws the reason a file is refused', async () => {
    answer(
      422,
      '{"error":{"code":"not_an_actual_export","message":"This isn\'t an Actual export."}}',
    )
    const refused = uploadActualExport(new File(['hello'], 'notes.txt'))
    await expect(refused).rejects.toBeInstanceOf(ApiError)
    await expect(refused).rejects.toThrow("This isn't an Actual export.")
  })
})

describe('getActualImport', () => {
  const plan = {
    id: 'abc',
    budget_name: 'Example Household',
    accounts: [],
    categories: [],
    app_accounts: [],
  }

  it('asks for the upload by its id', async () => {
    answer(200, JSON.stringify(plan))
    expect(await getActualImport('abc')).toEqual(plan)
    expect(sent().method).toBe('GET')
    expect(new URL(sent().url).pathname).toBe('/api/v1/imports/actual/abc')
  })

  it('throws once the upload is gone', async () => {
    answer(404, '{"error":{"code":"upload_not_found","message":"This upload is gone."}}')
    await expect(getActualImport('abc')).rejects.toThrow('This upload is gone.')
  })
})

describe('commitActualImport', () => {
  const request = { file_name: 'export.zip', accounts: [], categories: [] }

  it('sends the choices for the upload and resolves to the summary', async () => {
    const done = {
      batch_id: 1,
      accounts_created: 0,
      accounts: [],
      mapped: 0,
      undecided: 0,
      transfer: 0,
      uncategorized: 0,
      linked_transfers: 0,
    }
    answer(201, JSON.stringify(done))
    expect(await commitActualImport('abc', request)).toEqual(done)
    expect(sent().method).toBe('POST')
    expect(new URL(sent().url).pathname).toBe('/api/v1/imports/actual/abc/commit')
    expect(await sent().json()).toEqual(request)
  })

  it('throws while an earlier import is in place', async () => {
    answer(409, '{"error":{"code":"earlier_import","message":"Undo the earlier import first."}}')
    await expect(commitActualImport('abc', request)).rejects.toThrow(
      'Undo the earlier import first.',
    )
  })
})
