import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { mkdir, readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const origin = process.env.BC_BROWSER_ORIGIN;
const token = process.env.BC_BROWSER_TOKEN;
assert.ok(origin && token, 'The live Rust/PostgreSQL fixture must supply the origin and token.');
const browser = await chromium.launch({ channel: process.env.BC_BROWSER_CHANNEL });
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  const violations = [];
  const errors = [];
  const requests = [];
  await context.addInitScript(() => {
    window.cspViolations = [];
    document.addEventListener('securitypolicyviolation', (event) => window.cspViolations.push(event.violatedDirective));
  });
  page.on('pageerror', () => errors.push('Browser runtime error'));
  page.on('request', (request) => requests.push(request.url()));
  const storefront = await page.goto(origin);
  const csp = storefront.headers()['content-security-policy'];
  for (const directive of ["default-src 'self'", "connect-src 'self'", "object-src 'none'", "base-uri 'self'", "frame-ancestors 'none'"]) assert.ok(csp.includes(directive));
  assert.ok(!csp.includes('unsafe-inline') && !csp.includes('unsafe-eval'));
  await page.getByText('Connected to the shop', { exact: true }).waitFor();
  assert.equal((await page.request.get(`${origin}/readyz`)).status(), 200);
  await page.getByRole('button', { name: 'Check again' }).click();
  await page.getByText('Connected to the shop', { exact: true }).waitFor();
  violations.push(...await page.evaluate(() => window.cspViolations));

  await page.getByRole('link', { name: 'Merchant admin' }).click();
  await page.getByLabel('Installation token').fill('invalid-token');
  await page.getByRole('button', { name: 'Open admin' }).click();
  await page.getByRole('alert').filter({ hasText: 'not accepted' }).waitFor();
  assert.equal(await page.getByLabel('Installation token').inputValue(), '');
  await page.getByLabel('Installation token').fill(token);
  const statusResponse = page.waitForResponse((response) => response.url() === `${origin}/api/v1/admin/status` && response.status() === 200);
  await page.getByRole('button', { name: 'Open admin' }).click();
  assert.equal((await statusResponse).headers()['cache-control'], 'no-store');
  await page.getByRole('heading', { name: 'Welcome to your workspace.' }).waitFor();
  await page.getByRole('link', { name: 'Products', exact: true }).click();
  await page.getByRole('heading', { name: 'Products', exact: true }).waitFor();
  const storage = await page.evaluate(async () => ({ local: { ...localStorage }, session: { ...sessionStorage }, cookies: document.cookie, databases: await indexedDB.databases(), caches: await caches.keys() }));
  assert.deepEqual(storage, { local: {}, session: {}, cookies: '', databases: [], caches: [] });
  assert.ok(!await page.content().then((html) => html.includes(token)));
  await page.reload();
  await page.getByLabel('Installation token').waitFor();
  assert.equal(await page.getByLabel('Installation token').inputValue(), '');
  await page.getByLabel('Installation token').fill(token);
  await page.getByRole('button', { name: 'Open admin' }).click();
  await page.getByRole('heading', { name: 'Products', exact: true }).waitFor();
  await page.getByRole('button', { name: 'Forget token' }).click();
  await page.getByLabel('Installation token').waitFor();
  violations.push(...await page.evaluate(() => window.cspViolations));

  assert.deepEqual(violations, []);
  assert.deepEqual(errors, []);
  assert.ok(requests.every((url) => url.startsWith(`${origin}/`) && !url.includes(token)));
  // Verify both independently built trees contain no runtime credential.
  async function scan(directory) {
    for (const item of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, item.name);
      if (item.isDirectory()) await scan(path);
      else assert.ok(!(await readFile(path, 'utf8')).includes(token), 'Credential found in production assets');
    }
  }
  await scan(fileURLToPath(new URL('../admin/dist', import.meta.url)));
  await scan(fileURLToPath(new URL('../storefront/dist', import.meta.url)));
  // One batched desktop/mobile visual pass, with no credential in screenshots.
  const output = fileURLToPath(new URL('../../output/playwright', import.meta.url));
  await mkdir(output, { recursive: true });
  for (const [name, viewport] of [['desktop', { width: 1440, height: 1000 }], ['mobile', { width: 390, height: 844 }]]) {
    await page.setViewportSize(viewport);
    for (const [surface, path] of [['storefront', '/'], ['admin', '/admin']]) {
      await page.goto(`${origin}${path}`);
      if (surface === 'storefront') await page.getByText('Connected to the shop', { exact: true }).waitFor();
      else await page.getByLabel('Installation token').waitFor();
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await page.screenshot({ path: join(output, `${surface}-${name}.png`), fullPage: true });
    }
  }
  console.log('Live same-origin production smoke passed: Astro hydration, CSP, admin authority, navigation, memory-only token, reload, redaction, desktop/mobile.');
} finally {
  await browser.close();
}
