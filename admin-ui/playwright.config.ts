import { defineConfig, devices } from '@playwright/test';

const testPort = process.env.ADMIN_UI_TEST_PORT ?? '3721';
const testBaseURL = `http://127.0.0.1:${testPort}`;

export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  retries: 1,
  workers: 1,
  reporter: [['html', { open: 'never' }]],
  webServer: {
    command: `vx npm run dev -- --port ${testPort}`,
    url: `${testBaseURL}/admin/`,
    reuseExistingServer: !process.env.CI && !process.env.ADMIN_UI_TEST_PORT,
    timeout: 60_000,
  },
  use: {
    baseURL: testBaseURL,
    screenshot: 'only-on-failure',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
