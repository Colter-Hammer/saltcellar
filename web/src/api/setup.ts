import { call, client } from './client'
import type { components } from './generated/schema'

/** `GET /api/v1/setup`: whether setup is finished. */
export async function isSetupDone(): Promise<boolean> {
  return (await call(client.GET('/api/v1/setup'))).done
}

/** `POST /api/v1/setup/code`: resolves if `code` is the setup code; throws an ApiError if not. */
export async function checkSetupCode(code: string): Promise<void> {
  await call(client.POST('/api/v1/setup/code', { body: { code } }))
}

/** `GET /api/v1/setup/currencies`: the ISO 4217 codes the household can choose, sorted. */
export async function getCurrencies(): Promise<string[]> {
  return call(client.GET('/api/v1/setup/currencies'))
}

export type FinishSetup = components['schemas']['FinishSetup']

/**
 * `POST /api/v1/setup`: writes the household and the first login, and logs it in (the server sets
 * the session cookie). Throws an ApiError naming each wrong field, or a wrong code.
 */
export async function finishSetup(body: FinishSetup): Promise<void> {
  await call(client.POST('/api/v1/setup', { body }))
}
