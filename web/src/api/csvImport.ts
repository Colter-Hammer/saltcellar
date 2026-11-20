import { call, client } from './client'
import type { components } from './generated/schema'

export type CsvMapping = components['schemas']['CsvMapping']
export type CsvDateFormat = components['schemas']['CsvDateFormat']
export type CsvRead = components['schemas']['CsvRead']
export type CsvPreview = components['schemas']['CsvPreview']
export type CsvImportDone = components['schemas']['CsvImportDone']

/**
 * `POST /api/v1/imports/csv/read`: sends a file's text and resolves to its first records, the
 * mapping saved for its header on this account (null the first time) and a guess. Throws an
 * ApiError `empty_file` for a file with nothing in it.
 */
export async function readCsvFile(accountId: number, content: string): Promise<CsvRead> {
  return call(client.POST('/api/v1/imports/csv/read', { body: { account_id: accountId, content } }))
}

/**
 * `POST /api/v1/imports/csv/preview`: what Import would do with the file and mapping; nothing is
 * written. Throws an ApiError `invalid_mapping` for a column the file doesn't have.
 */
export async function previewCsvImport(
  accountId: number,
  content: string,
  mapping: CsvMapping,
): Promise<CsvPreview> {
  return call(
    client.POST('/api/v1/imports/csv/preview', {
      body: { account_id: accountId, content, mapping },
    }),
  )
}

/**
 * `POST /api/v1/imports/csv/commit`: imports the file as one import batch and saves the mapping.
 * `keep` lists the rows to check (their `row`) that should be added; the rest are skipped. Throws
 * an ApiError `nothing_readable` when no row reads with the mapping.
 */
export async function commitCsvImport(request: {
  accountId: number
  fileName: string
  content: string
  mapping: CsvMapping
  keep: number[]
}): Promise<CsvImportDone> {
  return call(
    client.POST('/api/v1/imports/csv/commit', {
      body: {
        account_id: request.accountId,
        file_name: request.fileName,
        content: request.content,
        mapping: request.mapping,
        keep: request.keep,
      },
    }),
  )
}
