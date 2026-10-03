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

// The cookie that keeps `token`, named after the page's port as the page names it.
function tokenCookie(baseUrl: string | undefined, token: string): string {
  return `explore_token_${new URL(baseUrl!).port}=${token}`;
}

test('the page refuses a request without the token of its address', async ({ explore, app }) => {
  expect(await status(app.baseUrl, '/?token=wrong')).toBe(403);
  expect(await status(app.baseUrl, '/')).toBe(403);
  expect(await status(app.baseUrl, '/status')).toBe(403);
  expect(await status(app.baseUrl, '/', { headers: { cookie: tokenCookie(app.baseUrl, 'wrong') } })).toBe(403);
  expect(await status(app.baseUrl, `/?token=${explore.token}`)).toBe(303);
});

// Browsers keep a host's cookies for all its ports: the token cookie of another page on this
// host, on another port, does not stand in for this page's own.
test('the page reads the token only from the cookie of its own port', async ({ explore, app }) => {
  expect(await status(app.baseUrl, '/', { headers: { cookie: `explore_token=${explore.token}` } })).toBe(403);
  expect(await status(app.baseUrl, '/', { headers: { cookie: `explore_token_1=${explore.token}` } })).toBe(403);
  expect(await status(app.baseUrl, '/', { headers: { cookie: tokenCookie(app.baseUrl, explore.token) } })).toBe(200);
});

test('every post of the page refuses a request without the token', async ({ explore, app }) => {
  const posts = ['/answer', '/pick', '/start', '/implement', '/quiz', '/quiz/skip', '/diagram-errors'];
  for (const path of posts) {
    expect(await status(app.baseUrl, path, { method: 'POST' })).toBe(403);
    const cookie = tokenCookie(app.baseUrl, 'wrong');
    expect(await status(app.baseUrl, path, { method: 'POST', headers: { cookie } })).toBe(403);
  }
  expect(await explore.answers()).toEqual([]);
  expect(await explore.starts()).toEqual([]);
  expect(await explore.implementations()).toEqual([]);
});

test('the page refuses a request for another host name', async ({ explore, app }) => {
  const host = `rebound.example:${new URL(app.baseUrl!).port}`;
  expect(await status(app.baseUrl, `/?token=${explore.token}`, { headers: { host } })).toBe(403);
  const cookie = tokenCookie(app.baseUrl, explore.token);
  expect(await status(app.baseUrl, '/', { headers: { host, cookie } })).toBe(403);
  expect(await status(app.baseUrl, '/test/sessions', { method: 'POST', headers: { host } })).toBe(403);
});

test('the page refuses a post from another site', async ({ app }) => {
  const origin = 'http://foreign.example';
  expect(await status(app.baseUrl, '/csp-report', { method: 'POST', headers: { origin } })).toBe(403);
  expect(await status(app.baseUrl, '/test/sessions', { method: 'POST', headers: { origin } })).toBe(403);
});

test('the page and its status refuse a script of another site', async ({ explore, app }) => {
  const cookie = tokenCookie(app.baseUrl, explore.token);
  const origin = 'http://127.0.0.1:1';
  expect(await status(app.baseUrl, '/', { headers: { cookie } })).toBe(200);
  expect(await status(app.baseUrl, '/status', { headers: { cookie } })).toBe(200);
  expect(await status(app.baseUrl, '/', { headers: { cookie, origin } })).toBe(403);
  expect(await status(app.baseUrl, '/status', { headers: { cookie, origin } })).toBe(403);
});
