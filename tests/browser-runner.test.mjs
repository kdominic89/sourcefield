/** Verify the portable runner's local-file and failure-cleanup boundaries without Playwright. */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { options, serveDocs } from '../scripts/verify-browser.mjs';

/** Capture an isolated command and fail promptly if leaked resources prevent process termination. */
function command(args) {
    return new Promise((resolve, reject) => {
        const child = spawn(process.execPath, args, { timeout: 5000 });
        let stderr = '';
        child.stderr.on('data', chunk => { stderr += chunk; });
        child.on('error', reject);
        child.on('exit', (code, signal) => resolve({ code, signal, stderr }));
    });
}

test('browser runner resolves explicit overrides and rejects unknown options', () => {
    const settings = options(['--output', './evidence'], {
        SOURCEFIELD_PLAYWRIGHT_MODULE: '/runtime/playwright/index.mjs',
        SOURCEFIELD_CHROMIUM: '/runtime/chromium',
    });

    assert.equal(settings['playwright-module'], '/runtime/playwright/index.mjs');
    assert.equal(settings.browser, '/runtime/chromium');
    assert.equal(settings.output, './evidence');
    assert.throws(() => options(['--unknown'], {}));
});

test('loopback server serves WASM and rejects escapes, directories and mutation methods', async () => {
    const temporary = await mkdtemp(join(tmpdir(), 'sourcefield-browser-server-'));
    const docs = join(temporary, 'docs');
    await mkdir(docs);
    await writeFile(join(docs, 'index.html'), '<title>Fixture</title>');
    await writeFile(join(docs, 'module.wasm'), Buffer.from([0, 97, 115, 109]));
    await writeFile(join(temporary, 'outside.txt'), 'outside');
    await symlink(join(temporary, 'outside.txt'), join(docs, 'escape.txt'));
    const server = await serveDocs(docs);

    try {
        const page = await fetch(server.url);
        const wasm = await fetch(`${server.url}/module.wasm`);
        const head = await fetch(`${server.url}/index.html`, { method: 'HEAD' });
        const escape = await fetch(`${server.url}/escape.txt`);
        const missing = await fetch(`${server.url}/%2e%2e%2foutside.txt`);
        const mutation = await fetch(server.url, { method: 'POST' });

        assert.match(server.url, /^http:\/\/127\.0\.0\.1:\d+$/);
        assert.equal(await page.text(), '<title>Fixture</title>');
        assert.equal(wasm.headers.get('content-type'), 'application/wasm');
        assert.equal(await head.text(), '');
        assert.equal(escape.status, 403);
        assert.ok([403, 404].includes(missing.status));
        assert.equal(mutation.status, 405);
    } finally {
        await server.close();
        await rm(temporary, { recursive: true });
    }
});

test('missing WASM fails clearly and failed browser launch closes the local server', async () => {
    const temporary = await mkdtemp(join(tmpdir(), 'sourcefield-browser-failure-'));
    const scripts = join(temporary, 'scripts');
    const pkg = join(temporary, 'docs/pkg');
    const output = join(temporary, 'evidence');
    await mkdir(scripts);
    await copyFile(new URL('../scripts/verify-browser.mjs', import.meta.url), join(scripts, 'verify-browser.mjs'));
    const args = [join(scripts, 'verify-browser.mjs'), '--output', output, '--site', join(temporary, 'docs')];

    try {
        const missingWasm = await command(args);

        assert.equal(missingWasm.code, 1);
        assert.match(missingWasm.stderr, /Built WASM is required/);
        await mkdir(pkg, { recursive: true });
        await writeFile(join(pkg, 'sourcefield_wasm.js'), '');
        await writeFile(join(pkg, 'sourcefield_wasm_bg.wasm'), '');

        const missingRuntime = await command([...args, '--playwright-module', join(temporary, 'missing.mjs')]);

        assert.equal(missingRuntime.code, 1);
        assert.match(missingRuntime.stderr, /Playwright is unavailable/);
        const runtime = join(temporary, 'runtime.mjs');
        await writeFile(runtime, 'export const chromium = { launch: async () => { throw Error("fixture launch failed"); } };');

        const failedLaunch = await command([...args, '--playwright-module', runtime]);
        const report = JSON.parse(await readFile(join(output, 'results.json'), 'utf8'));

        assert.equal(failedLaunch.code, 1);
        assert.equal(failedLaunch.signal, null);
        assert.match(failedLaunch.stderr, /fixture launch failed/);
        assert.equal(report.passed, false);
    } finally {
        await rm(temporary, { recursive: true });
    }
});
