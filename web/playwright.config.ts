// End-to-end tests (docs/standards.md#tests), run against the app image: `just e2e` starts it with
// compose, CI with `docker run`. Both pass its address in E2E_BASE_URL, a command that prints its
// log in E2E_APP_LOGS, where the setup test finds the setup code, and a command that runs `budget`
// in it in E2E_APP_EXEC, for the reset link.
import { defineConfig, devices } from '@playwright/test'
import { STORAGE_STATE } from './e2e/login-state.js'

export default defineConfig({
  testDir: 'e2e',
  forbidOnly: !!process.env.CI,
  // No retries: the setup test claims the install once, so a retry would find it claimed.
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8080',
    trace: 'retain-on-failure',
  },
  projects: [
    // The fresh install is claimed first, through the setup wizard, on a phone.
    { name: 'setup', testMatch: 'setup.spec.ts', use: { ...devices['Pixel 7'] } },
    // Then the reset link and the profile, on their own: they end sessions, the shared one among
    // them, and leave a new one in STORAGE_STATE.
    {
      name: 'profile',
      testMatch: 'profile.spec.ts',
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then the categories, on their own too: the list is the install's, and the test works it on
    // both widths in turn.
    {
      name: 'categories',
      testMatch: 'categories.spec.ts',
      dependencies: ['profile'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Then the two layouts: a phone (tab bar) and a desktop (sidebar), both in Chromium.
    {
      name: 'phone',
      testIgnore: [
        'setup.spec.ts',
        'profile.spec.ts',
        'categories.spec.ts',
        'import-commit.spec.ts',
      ],
      dependencies: ['categories'],
      use: { ...devices['Pixel 7'], storageState: STORAGE_STATE },
    },
    {
      name: 'desktop',
      testIgnore: [
        'setup.spec.ts',
        'profile.spec.ts',
        'categories.spec.ts',
        'import-commit.spec.ts',
      ],
      dependencies: ['categories'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
    // Last, the import itself, once: while it's in place every other upload is refused, and the
    // install has accounts.
    {
      name: 'import-commit',
      testMatch: 'import-commit.spec.ts',
      dependencies: ['phone', 'desktop'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
  ],
})
