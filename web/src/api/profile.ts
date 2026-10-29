import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { call, client } from './client'
import type { components } from './generated/schema'

export type ChangeProfile = components['schemas']['ChangeProfile']
export type ChangePassword = components['schemas']['ChangePassword']

const PROFILE = ['profile']

/** `GET /api/v1/profile`: the logged-in login's name, locale and week start. */
export function useProfile() {
  return useQuery({
    queryKey: PROFILE,
    queryFn: () => call(client.GET('/api/v1/profile')),
    staleTime: Infinity,
  })
}

/**
 * `PUT /api/v1/profile`: saves the name, locale and week start. The saved profile replaces the one
 * every screen formats with, so they change at once. Throws an ApiError naming each wrong field.
 */
export function useChangeProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: ChangeProfile) => call(client.PUT('/api/v1/profile', { body })),
    onSuccess: (saved) => {
      queryClient.setQueryData(PROFILE, saved)
    },
  })
}

/**
 * `POST /api/v1/profile/password`: changes the password. Throws an ApiError naming each wrong
 * field, the current password among them, or while the throttle pauses it (`too_many_attempts`).
 */
export async function changePassword(body: ChangePassword): Promise<void> {
  await call(client.POST('/api/v1/profile/password', { body }))
}

/** `DELETE /api/v1/sessions`: ends every session this login has, this one included. */
export async function logOutEverywhere(): Promise<void> {
  // Every mutating request is JSON, body or not (docs/spec/foundations.md#http-api).
  await call(client.DELETE('/api/v1/sessions', { headers: { 'Content-Type': 'application/json' } }))
}
