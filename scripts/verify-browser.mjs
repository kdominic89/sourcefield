#!/usr/bin/env node
/** Run real Chromium regressions against the generated local site without installing tools. */
import { createServer } from 'node:http';
import { access, mkdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { dirname, extname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const types = {
    '.html': 'text/html; charset=utf-8',
    '.css': 'text/css; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.json': 'application/json',
    '.svg': 'image/svg+xml',
    '.wasm': 'application/wasm',
    '.webmanifest': 'application/manifest+json',
};

/** Serve only real files beneath docs on an ephemeral loopback port, including WASM MIME types. */
export async function serveDocs(directory) {
    const base = await realpath(directory);
    const server = createServer(async (request, response) => {
        if (!['GET', 'HEAD'].includes(request.method)) {
            response.writeHead(405).end();
            return;
        }

        try {
            const pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
            const candidate = resolve(base, `.${pathname === '/' ? '/index.html' : pathname}`);
            const file = await realpath(candidate);
            const within = relative(base, file);

            // Resolve symlinks before containment checks so the preview cannot expose sibling files.
            if (within.startsWith(`..${sep}`) || within === '..' || isAbsolute(within)) {
                response.writeHead(403).end();
                return;
            }

            if (!(await stat(file)).isFile()) {
                response.writeHead(404).end();
                return;
            }

            const body = await readFile(file);
            response.writeHead(200, {
                'Content-Type': types[extname(file)] ?? 'application/octet-stream',
                'Content-Length': body.length,
                'Cache-Control': 'no-store',
            });
            response.end(request.method === 'HEAD' ? undefined : body);
        } catch {
            response.writeHead(404).end();
        }
    });

    await new Promise((resolveReady, reject) => {
        server.once('error', reject);
        server.listen(0, '127.0.0.1', resolveReady);
    });

    return {
        url: `http://127.0.0.1:${server.address().port}`,
        /** Release keep-alive sockets as well as the listening endpoint. */
        async close() {
            server.closeAllConnections();
            await new Promise((resolveClosed, reject) => server.close(error => error ? reject(error) : resolveClosed()));
        },
    };
}

/** Resolve local runtime overrides without downloading a dependency or assuming a user home path. */
export function options(args = process.argv.slice(2), environment = process.env) {
    const { values } = parseArgs({ args, options: {
        'playwright-module': { type: 'string', default: environment.SOURCEFIELD_PLAYWRIGHT_MODULE ?? 'playwright' },
        browser: { type: 'string', default: environment.SOURCEFIELD_CHROMIUM },
        site: { type: 'string', default: join(root, 'dist/site') },
        output: { type: 'string', default: join(root, 'dist/browser-checks') },
        help: { type: 'boolean', default: false },
    } });

    return values;
}

/** Load the existing library and require genuine WASM before beginning the end-to-end checks. */
async function run(settings) {
    for (const name of ['sourcefield_wasm.js', 'sourcefield_wasm_bg.wasm']) {
        try {
            await access(join(resolve(settings.site), 'pkg', name));
        } catch {
            throw new Error('Built WASM is required. Run ./scripts/build-wasm.sh before browser verification.');
        }
    }

    const moduleName = settings['playwright-module'];
    const specifier = isAbsolute(moduleName) || moduleName.startsWith('.')
        ? pathToFileURL(resolve(moduleName)).href : moduleName;
    let playwright;

    try {
        playwright = await import(specifier);
    } catch (cause) {
        throw new Error('Playwright is unavailable. Set --playwright-module to an installed library entry file.', { cause });
    }

    const chromium = playwright.chromium ?? playwright.default?.chromium;

    if (!chromium?.launch) {
        throw new Error('The supplied Playwright module does not export chromium.launch.');
    }

    const output = resolve(settings.output);
    await mkdir(output, { recursive: true });
    const server = await serveDocs(resolve(settings.site));
    let browser;
    const report = { passed: false, checks: {} };

    try {
        browser = await chromium.launch({ executablePath: settings.browser, headless: true, chromiumSandbox: true });
        const { runInteractionChecks } = await import('../tests/integration/interactions.mjs');
        const { runFieldChecks } = await import('../tests/integration/field.mjs');
        const { runSimulationChecks } = await import('../tests/integration/simulation.mjs');
        const context = { browser, base: server.url, output };

        report.checks.simulation = await runSimulationChecks(context);
        report.checks.interactions = await runInteractionChecks(context);
        report.checks.field = await runFieldChecks(context);
        report.passed = true;
        console.log(`Browser verification passed. Evidence: ${output}`);
    } catch (error) {
        report.error = error.message;
        throw error;
    } finally {
        // A failed assertion or browser launch must not leave a listening preview server behind.
        try {
            await browser?.close();
        } finally {
            await server.close();
            await writeFile(join(output, 'results.json'), `${JSON.stringify(report, null, 2)}\n`);
        }
    }
}

/** Print the portable command contract or run the suite with a failing process exit on error. */
async function main() {
    const settings = options();

    if (settings.help) {
        console.log('Usage: node scripts/verify-browser.mjs [--playwright-module FILE] [--browser FILE] [--output DIR] [--site DIR]');
        console.log('Uses an existing Playwright library and Chromium; serves docs only on an ephemeral loopback port.');
        return;
    }

    await run(settings);
}

if (process.argv[1] && await realpath(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main().catch(error => {
        console.error(`Browser verification failed: ${error.message}`);
        process.exitCode = 1;
    });
}
