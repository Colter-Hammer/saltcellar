import { call, client } from './client'
import type { components } from './generated/schema'

export type ActualUpload = components['schemas']['ActualUpload']

/**
 * `POST /api/v1/imports/actual`: sends an Actual export, the `.zip` itself, and resolves to what's
 * in it. Nothing is imported yet. Throws an ApiError saying why a file is refused: not an Actual
 * export, over 100 MB, the household currency, or an earlier import still in place.
 */
export async function uploadActualExport(file: Blob): Promise<ActualUpload> {
  return call(
    client.POST('/api/v1/imports/actual', {
      // The body is the file as is; the generated type can only call it a string.
      body: '',
      bodySerializer: () => file,
      headers: { 'Content-Type': 'application/zip' },
    }),
  )
}
