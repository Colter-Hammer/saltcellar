import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './client'
import { commitCsvImport, previewCsvImport, readCsvFile } from './csvImport'
import type { CsvMapping } from './csvImport'

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

const mapping: CsvMapping = {
  date: 0,
  post_date: null,
  description: 1,
  amount: { kind: 'signed', column: 2 },
  flip_sign: false,
  date_format: 'ymd',
  category: null,
  memo: null,
  skip_rows: 1,
}

describe('readCsvFile', () => {
  it('sends the account and the text and resolves to what is in it', async () => {
    const read = { records: [['Date']], record_count: 1, columns: 1, saved: null, guess: mapping }
    answer(200, JSON.stringify(read))
    expect(await readCsvFile(7, 'Date\n')).toEqual(read)
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/csv/read',
      body: { account_id: 7, content: 'Date\n' },
    })
  })

  it('throws why a file is refused', async () => {
    answer(422, '{"error":{"code":"empty_file","message":"There\'s nothing in this file."}}')
    await expect(readCsvFile(7, '')).rejects.toBeInstanceOf(ApiError)
    await expect(readCsvFile(7, '')).rejects.toThrow("There's nothing in this file.")
  })
})

describe('previewCsvImport', () => {
  it('sends the mapping with the file', async () => {
    const preview = {
      new: 1,
      already: 0,
      to_check: 0,
      unreadable: 0,
      sample: [],
      check: [],
      problems: [],
    }
    answer(200, JSON.stringify(preview))
    expect(await previewCsvImport(7, 'a,b', mapping)).toEqual(preview)
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/csv/preview',
      body: { account_id: 7, content: 'a,b', mapping },
    })
  })
})

describe('commitCsvImport', () => {
  it('sends the decisions and resolves to the batch', async () => {
    const done = { batch_id: 3, added: 2, matched: 0, skipped: 0 }
    answer(201, JSON.stringify(done))
    const request = { accountId: 7, fileName: 'a.csv', content: 'a,b', mapping, keep: [4] }
    expect(await commitCsvImport(request)).toEqual(done)
    expect(await sentBody()).toEqual({
      path: '/api/v1/imports/csv/commit',
      body: { account_id: 7, file_name: 'a.csv', content: 'a,b', mapping, keep: [4] },
    })
  })
})
