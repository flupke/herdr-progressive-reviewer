import { request } from 'node:http';
import { expect } from 'e2e';
import { test } from './session.ts';

// node:http, unlike fetch, sends the Host and Origin headers it is given.
function status(
  baseUrl: string | undefined,
  path: string,
  options: { method?: string; headers?: Record<string, string> } = {},
): Promise<number> {
  return new Promise((resolve, reject) => {
    request(new URL(path, baseUrl), options, (response) => {
      response.resume();
      resolve(response.statusCode!);
    })
      .on('error', reject)
      .end();
  });
}

test('the page refuses a request without the token of its address', async ({ explore, app }) => {
  expect(await status(app.baseUrl, '/?token=wrong')).toBe(403);
  expect(await status(app.baseUrl, '/')).toBe(403);
  expect(await status(app.baseUrl, '/status')).toBe(403);
  expect(await status(app.baseUrl, '/', { headers: { cookie: 'explore_token=wrong' } })).toBe(403);
  expect(await status(app.baseUrl, `/?token=${explore.token}`)).toBe(303);
});

test('the page refuses a request for another host name', async ({ explore, app }) => {
  const host = `rebound.example:${new URL(app.baseUrl!).port}`;
  expect(await status(app.baseUrl, `/?token=${explore.token}`, { headers: { host } })).toBe(403);
  const cookie = `explore_token=${explore.token}`;
  expect(await status(app.baseUrl, '/', { headers: { host, cookie } })).toBe(403);
  expect(await status(app.baseUrl, '/test/sessions', { method: 'POST', headers: { host } })).toBe(403);
});

test('the page refuses a post from another site', async ({ app }) => {
  const origin = 'http://foreign.example';
  expect(await status(app.baseUrl, '/csp-report', { method: 'POST', headers: { origin } })).toBe(403);
  expect(await status(app.baseUrl, '/test/sessions', { method: 'POST', headers: { origin } })).toBe(403);
});

test('the page and its status refuse a script of another site', async ({ explore, app }) => {
  const cookie = `explore_token=${explore.token}`;
  const origin = 'http://127.0.0.1:1';
  expect(await status(app.baseUrl, '/', { headers: { cookie } })).toBe(200);
  expect(await status(app.baseUrl, '/status', { headers: { cookie } })).toBe(200);
  expect(await status(app.baseUrl, '/', { headers: { cookie, origin } })).toBe(403);
  expect(await status(app.baseUrl, '/status', { headers: { cookie, origin } })).toBe(403);
});
