import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

export type ActualUpload = components['schemas']['ActualUpload']
export type ActualImport = components['schemas']['ActualImport']
export type ActualAccount = components['schemas']['ActualAccount']
export type ActualCategory = components['schemas']['ActualCategory']
export type AccountType = components['schemas']['AccountType']
export type AccountRole = components['schemas']['AccountRole']
export type ActualCommitRequest = components['schemas']['ActualCommitRequest']
export type ActualImportDone = components['schemas']['ActualImportDone']
export type ActualPreview = components['schemas']['ActualPreview']

/**
 * `POST /api/v1/imports/actual`: sends an Actual export, the `.zip` itself, and resolves to what's
 * in it. Nothing is imported yet. Throws an ApiError saying why a file is refused: not an Actual
 * export, over 100 MB, or the household currency.
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

/**
 * `POST /api/v1/imports/actual/{id}/preview`: what importing with these choices would do, writing
 * nothing: rows new and already there, and the possible duplicates to decide on. Throws as the
 * commit does.
 */
export async function previewActualImport(
  id: string,
  body: ActualCommitRequest,
): Promise<ActualPreview> {
  return call(
    client.POST('/api/v1/imports/actual/{id}/preview', { params: { path: { id } }, body }),
  )
}

/**
 * `POST /api/v1/imports/actual/{id}/commit`: imports the upload with the choices of steps 2 and 3,
 * as one import batch, and resolves to the Done step's summary. Throws an ApiError
 * `invalid_choices` for choices that can't go ahead, and `upload_not_found` once the upload is
 * gone.
 */
export async function commitActualImport(
  id: string,
  body: ActualCommitRequest,
): Promise<ActualImportDone> {
  return call(client.POST('/api/v1/imports/actual/{id}/commit', { params: { path: { id } }, body }))
}

export type ImportBatch = components['schemas']['ImportBatch']

/** `GET /api/v1/imports`: every import batch, active or undone, latest first. */
export function useImportBatches() {
  return useQuery({
    queryKey: ['import-batches'],
    queryFn: () => call(client.GET('/api/v1/imports')),
  })
}

/**
 * `POST /api/v1/imports/{id}/undo`: undoes a batch. Without `confirm`, an ApiError `rows_edited`
 * says rows were edited since and nothing was changed; asking again with `confirm` goes ahead.
 * Afterwards everything the import wrote is gone, so every screen reloads.
 */
export function useUndoImport() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, confirm }: { id: number; confirm: boolean }) =>
      call(
        client.POST('/api/v1/imports/{id}/undo', { params: { path: { id } }, body: { confirm } }),
      ),
    onSuccess: () => queryClient.invalidateQueries(),
  })
}
