import { call, client } from './client'
import type { components } from './generated/schema'

export type ActualUpload = components['schemas']['ActualUpload']
export type ActualImport = components['schemas']['ActualImport']
export type ActualAccount = components['schemas']['ActualAccount']
export type ActualCategory = components['schemas']['ActualCategory']
export type AccountType = components['schemas']['AccountType']
export type AccountRole = components['schemas']['AccountRole']

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

/**
 * `GET /api/v1/imports/actual/{id}`: the uploaded export's accounts and categories, with what each
 * choice starts from, and the app's accounts for Same as. Throws an ApiError `upload_not_found`
 * once the upload is gone.
 */
export async function getActualImport(id: string): Promise<ActualImport> {
  return call(client.GET('/api/v1/imports/actual/{id}', { params: { path: { id } } }))
}
