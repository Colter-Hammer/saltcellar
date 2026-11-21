import type { CsvImportDone, CsvPreview } from './csvImport'
import { call, client } from './client'
import type { components } from './generated/schema'

export type OfxRead = components['schemas']['OfxRead']

/**
 * `POST /api/v1/imports/ofx/read`: sends an OFX, QFX or QBO file's text and resolves to its
 * account's last 4 digits, the account they pre-select (null for none or several) and how many
 * transactions it holds. Throws an ApiError `not_ofx`, `empty_file` or `several_accounts`.
 */
export async function readOfxFile(content: string): Promise<OfxRead> {
  return call(client.POST('/api/v1/imports/ofx/read', { body: { content } }))
}

/** `POST /api/v1/imports/ofx/preview`: what Import would do with the file; nothing is written. */
export async function previewOfxImport(accountId: number, content: string): Promise<CsvPreview> {
  return call(
    client.POST('/api/v1/imports/ofx/preview', { body: { account_id: accountId, content } }),
  )
}

/**
 * `POST /api/v1/imports/ofx/commit`: imports the file as one import batch. `keep` lists the rows to
 * check (their `row`) that should be added. Throws an ApiError `nothing_readable` when no
 * transaction reads.
 */
export async function commitOfxImport(request: {
  accountId: number
  fileName: string
  content: string
  keep: number[]
}): Promise<CsvImportDone> {
  return call(
    client.POST('/api/v1/imports/ofx/commit', {
      body: {
        account_id: request.accountId,
        file_name: request.fileName,
        content: request.content,
        keep: request.keep,
      },
    }),
  )
}
