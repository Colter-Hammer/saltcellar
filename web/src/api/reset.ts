import { call, client } from './client'

/**
 * `POST /api/v1/password-reset/check`: resolves if the reset link's token still works; throws an
 * ApiError (`reset_link_dead`) if it expired, was used, or a newer link replaced it.
 */
export async function checkReset(token: string): Promise<void> {
  await call(client.POST('/api/v1/password-reset/check', { body: { token } }))
}

/**
 * `POST /api/v1/password-reset`: sets the new password and logs in (the server sets the session
 * cookie). Throws an ApiError naming each wrong field, or `reset_link_dead`.
 */
export async function resetPassword(
  token: string,
  password: string,
  passwordAgain: string,
): Promise<void> {
  await call(
    client.POST('/api/v1/password-reset', {
      body: { token, password, password_again: passwordAgain },
    }),
  )
}
