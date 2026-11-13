import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

/** One sync run, as Settings › Bank connection & sync shows it. */
export type SyncRun = components['schemas']['SyncRun']
/** The daily schedule, the request budget and Sync now's wait. */
export type SyncState = components['schemas']['SyncState']

/** One bank link of the connection and its state, as Settings › Bank connection & sync shows it. */
export type Bank = components['schemas']['Bank']

/**
 * `GET /api/v1/bank-connection`: whether the server has its secret key, and the household's bank
 * connection (null before a setup token is claimed). The connection's secret is never part of it.
 * Polled every minute, so the paused banner appears when a sync pauses the connection *(agent)*.
 */
export function useBankConnection() {
  return useQuery({
    queryKey: ['bank-connection'],
    queryFn: () => call(client.GET('/api/v1/bank-connection')),
    refetchInterval: 60_000,
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

/**
 * `POST /api/v1/bank-connection/sync-after-reconnect`: **Done, sync now**, once a bank has been
 * re-authorized at SimpleFIN. Like Sync now, but it skips the cooldown. Resolves to the run once
 * it has finished; throws an ApiError `not_connected`, `sync_paused` or `sync_limit_reached`. The
 * banks, accounts and rows reload either way.
 */
export function useSyncAfterReconnect() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      call(
        client.POST('/api/v1/bank-connection/sync-after-reconnect', {
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

/**
 * `PUT /api/v1/bank-connection/sync-settings`: the daily sync time (`HH:MM`) and whether raw bank
 * responses are kept; resolves to the connection screen. Throws an ApiError `validation_failed`
 * naming `sync_time`. Turning raw responses off deletes the kept ones.
 */
export function useChangeSyncSettings() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (settings: { sync_time: string; keep_raw_responses: boolean }) =>
      call(client.PUT('/api/v1/bank-connection/sync-settings', { body: settings })),
    onSuccess: (connection) => {
      queryClient.setQueryData(['bank-connection'], connection)
    },
  })
}

/** `GET /api/v1/bank-connection/sync-runs`: Recent syncs, newest first, 90 days of them. */
export function useSyncHistory() {
  return useQuery({
    queryKey: ['bank-connection', 'sync-runs'],
    queryFn: () => call(client.GET('/api/v1/bank-connection/sync-runs')),
  })
}
