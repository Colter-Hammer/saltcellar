// End-to-end tests (docs/standards.md#tests), run against the app image: `just e2e` starts it with
// compose, CI with `docker run`. Both pass its address in E2E_BASE_URL, and a command that prints
// its log in E2E_APP_LOGS, where the setup test finds the setup code.
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
    // The fresh install is claimed first, through the setup wizard.
    { name: 'setup', testMatch: 'setup.spec.ts', use: { ...devices['Desktop Chrome'] } },
    // Then the two layouts: a phone (tab bar) and a desktop (sidebar), both in Chromium.
    {
      name: 'phone',
      testIgnore: 'setup.spec.ts',
      dependencies: ['setup'],
      use: { ...devices['Pixel 7'], storageState: STORAGE_STATE },
    },
    {
      name: 'desktop',
      testIgnore: 'setup.spec.ts',
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], storageState: STORAGE_STATE },
    },
  ],
})
