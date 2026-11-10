import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

/** One sync run, as Settings › Bank connection & sync shows it. */
export type SyncRun = components['schemas']['SyncRun']

/**
 * `GET /api/v1/bank-connection`: whether the server has its secret key, and the household's bank
 * connection (null before a setup token is claimed). The connection's secret is never part of it.
 */
export function useBankConnection() {
  return useQuery({
    queryKey: ['bank-connection'],
    queryFn: () => call(client.GET('/api/v1/bank-connection')),
  })
}

/**
 * `PUT /api/v1/bank-connection`: claims a setup token, a new connection or a replacement for the
 * existing one, and resolves to the connection. The claim also reads the connection's accounts
 * into the new accounts prompt. Throws an ApiError: `validation_failed` naming
 * `setup_token` if it isn't a setup token (nothing was asked of SimpleFIN), `secret_key_missing`,
 * `token_refused` if SimpleFIN won't take it, or `bridge_unavailable`.
 */
export function useClaimSetupToken() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (setupToken: string) =>
      call(client.PUT('/api/v1/bank-connection', { body: { setup_token: setupToken } })),
    onSuccess: async (connection) => {
      queryClient.setQueryData(['bank-connection'], connection)
      // The claim read the connection's accounts: the prompt fills, and some may have attached.
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['connection-accounts'] }),
        queryClient.invalidateQueries({ queryKey: ['accounts'] }),
      ])
    },
  })
}

/**
 * `DELETE /api/v1/bank-connection`: disconnects. The accounts stay and become manual ones, so
 * their screens reload.
 */
export function useDisconnectBank() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      // Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
      call(
        client.DELETE('/api/v1/bank-connection', {
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    onSuccess: () => queryClient.invalidateQueries(),
  })
}

/**
 * `POST /api/v1/bank-connection/sync`: Sync now. Resolves to the run once it has finished (the
 * running one, if a sync was already going); a run the Bridge failed resolves too, with its
 * `error`. Throws an ApiError `not_connected` or `sync_paused`. The rows, accounts and the
 * connection reload either way.
 */
export function useSyncNow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      call(
        client.POST('/api/v1/bank-connection/sync', {
          // Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ['bank-connection'] }),
        queryClient.invalidateQueries({ queryKey: ['connection-accounts'] }),
        queryClient.invalidateQueries({ queryKey: ['accounts'] }),
        queryClient.invalidateQueries({ queryKey: ['transactions'] }),
      ]),
  })
}
