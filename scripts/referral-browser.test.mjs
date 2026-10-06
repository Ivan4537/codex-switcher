import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source = fs.readFileSync('src-tauri/src/referral_browser.js', 'utf8');

function browser(identity, status = 200) {
    const calls = [];
    const elements = [];
    const document = {
        readyState: 'complete', body: { appendChild() {} }, getElementById: () => null,
        createElement: tag => { const node = { tag, style: {}, append() {} }; elements.push(node); return node; },
    };
    const location = { hostname: 'chatgpt.com', protocol: 'https:', href: '' };
    const context = {
        document, location, URL, URLSearchParams,
        fetch: async (url, options) => {
            calls.push({ url, options });
            return { ok: calls.length === 1 || status === 200, status,
                headers: new Map([['content-type', 'application/json']]),
                json: async () => calls.length === 1 ? identity
                    : { should_show: true, remaining_send_capacity: 3, grants: [] },
            };
        },
    };
    const config = { nonce: 'test-nonce', token: 'test-token', account_id: 'A', user_id: 'U', program: 'codex_referral_consumer' };
    vm.runInNewContext(`(() => { const CONFIG = ${JSON.stringify(config)}; ${source} })();`, context);
    return { calls, elements, location, button: elements.find(node => node.tag === 'button') };
}

test('browser route only GETs and binds callback to selected identity and nonce', async () => {
    const fixture = browser({ account_id: 'A', user_id: 'U' });
    await fixture.button.onclick();
    assert.equal(fixture.calls.length, 2);
    assert.ok(fixture.calls.every(call => call.options.method === 'GET' && call.options.redirect === 'error'));
    const result = new URL(fixture.location.href);
    assert.equal(result.searchParams.get('nonce'), 'test-nonce');
    const data = JSON.parse(result.searchParams.get('data'));
    assert.equal(data.offer.remaining_send_capacity, 3);
    assert.equal(data.account_id, 'A');
    assert.ok(!result.toString().includes('test-token'));
});

test('wrong account or a challenged query returns no confirmed result', async () => {
    for (const fixture of [browser({ account_id: 'B', user_id: 'U' }), browser({ account_id: 'A', user_id: 'U' }, 403)]) {
        await fixture.button.onclick();
        assert.equal(fixture.location.href, '');
        assert.equal(fixture.button.disabled, false);
    }
});
