// The app's container, reached through commands the runner passes in (`just e2e` and CI set them).
import { execSync, spawnSync } from 'node:child_process'

/** The app's log so far, through the command in E2E_APP_LOGS. */
export function appLog(): string {
  const command = process.env.E2E_APP_LOGS
  if (!command) throw new Error('E2E_APP_LOGS must be a command that prints the app’s log')
  return execSync(command, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
}

/** Runs the `budget` binary in the app's container with `args`, through E2E_APP_EXEC. */
export function budget(...args: string[]): {
  status: number | null
  stdout: string
  stderr: string
} {
  const command = process.env.E2E_APP_EXEC
  if (!command) throw new Error('E2E_APP_EXEC must be a command that runs `budget` in the app')
  const [program, ...rest] = command.split(' ')
  if (program === undefined) throw new Error('E2E_APP_EXEC is empty')
  const run = spawnSync(program, [...rest, ...args], { encoding: 'utf8' })
  return { status: run.status, stdout: run.stdout, stderr: run.stderr }
}
