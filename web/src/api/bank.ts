import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

export type BankConnection = components['schemas']['BankConnection']
export type ConnectionStatus = components['schemas']['ConnectionStatus']

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
 * existing one, and resolves to the connection. Throws an ApiError: `validation_failed` naming
 * `setup_token` if it isn't a setup token (nothing was asked of SimpleFIN), `secret_key_missing`,
 * `token_refused` if SimpleFIN won't take it, or `bridge_unavailable`.
 */
export function useClaimSetupToken() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (setupToken: string) =>
      call(client.PUT('/api/v1/bank-connection', { body: { setup_token: setupToken } })),
    onSuccess: (connection) => {
      queryClient.setQueryData(['bank-connection'], connection)
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
