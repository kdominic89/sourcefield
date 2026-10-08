import assert from 'node:assert/strict';
import { join } from 'node:path';
import { readFile } from 'node:fs/promises';

/**
 * Exercise keyboard, touch, WASM, responsive layout, and failure recovery in Chromium.
 * @param {{browser: import('playwright').Browser, base: string, output: string}} options Runtime and artifact location.
 * @returns {Promise<{results: object[]}>} Results for all interaction groups.
 */
export async function runInteractionChecks({ browser, base, output }) {
    const context = await browser.newContext({
        viewport: { width: 1500, height: 1100 },
        colorScheme: 'dark',
        hasTouch: true,
    });

    try {
        const results = [];
        const errors = [];
        const page = await context.newPage();
        page.on('pageerror', error => errors.push(error.message));
        page.on('console', message => {
            if (message.type() === 'error') {
                errors.push(message.text());
            }
        });

        await page.goto(base);
        await page.waitForFunction(() => document.querySelector('#engine-label').textContent.includes('RUST/WASM'));
        await page.waitForSelector('#profile-view svg');

        assert.ok(await page.locator('#profile-view [data-node-id]').count() > 0);
        results.push({
            check: 'native WASM and canonical SVG',
            engine: await page.locator('#engine-label').textContent(),
        });
        await page.screenshot({ path: join(output, 'browser-desktop.png'), fullPage: true });

        const normalMotion = await page.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).filter(animation => animation.playState === 'running').length);

        assert.ok(normalMotion > 0);
        await page.locator('#motion-button').click();

        assert.equal(await page.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).every(animation => animation.playState !== 'running')), true);
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.waitForFunction(() => document.querySelector('#motion-button').disabled);

        assert.equal(await page.locator('#motion-button').isDisabled(), true);
        await page.emulateMedia({ reducedMotion: 'no-preference' });
        await page.waitForFunction(() => !document.querySelector('#motion-button').disabled);

        assert.equal(await page.locator('#motion-button').getAttribute('aria-pressed'), 'true');
        results.push({ check: 'pause and live reduced-motion preserve user pause', animations: normalMotion });

        const chrome = await page.evaluate(async () => {
            const state = await (await fetch('./profile-state.json')).json();
            const style = getComputedStyle(document.documentElement);
            return {
                variant: state.profile.variant,
                applied: document.documentElement.dataset.profileVariant,
                primary: style.getPropertyValue('--personal').trim(),
                secondary: style.getPropertyValue('--personal-2').trim(),
            };
        });

        assert.equal(chrome.applied, chrome.variant);
        assert.equal(chrome.primary, chrome.variant === 'organization' ? '#ffb86b' : '#8b7cff');
        assert.equal(chrome.secondary, chrome.variant === 'organization' ? '#5ec8ff' : '#4de7c2');
        await page.emulateMedia({ colorScheme: 'light' });
        await page.waitForFunction(() => getComputedStyle(document.documentElement)
            .getPropertyValue('--personal').trim() ===
            (document.documentElement.dataset.profileVariant === 'organization' ? '#c66b16' : '#6657e8'));

        const lightSecondary = await page.evaluate(() => getComputedStyle(document.documentElement)
            .getPropertyValue('--personal-2').trim());

        assert.equal(lightSecondary, chrome.variant === 'organization' ? '#1d7fbc' : '#008e79');
        await page.emulateMedia({ colorScheme: 'dark' });
        await page.waitForFunction(() => getComputedStyle(document.documentElement)
            .getPropertyValue('--personal').trim() ===
            (document.documentElement.dataset.profileVariant === 'organization' ? '#ffb86b' : '#8b7cff'));
        results.push({ check: 'variant-owned personal and organization chrome in dark and light', ...chrome });

        const projectButton = page.locator('#node-list button').first();
        await page.locator('.semantic-profile summary').click();
        await projectButton.focus();
        await page.keyboard.press('Enter');

        assert.equal(await page.locator('#detail-panel').getAttribute('aria-hidden'), 'false');
        assert.equal(await page.locator('#detail-close').evaluate(element => document.activeElement === element), true);
        await page.keyboard.press('Escape');

        assert.equal(await page.locator('#detail-panel').getAttribute('aria-hidden'), 'true');
        assert.equal(await projectButton.evaluate(element => document.activeElement === element), true);

        const packageLinks = await page.locator('#profile-view a[href*="nuget.org"]')
            .evaluateAll(elements => elements.map(element => element.getAttribute('href')));
        const firstPackage = page.locator('#profile-view a[href*="nuget.org"]').first();

        const packageState = await (await context.request.get(new URL('profile-state.json', base).href)).json();
        const expectedPackages = packageState.nodes.filter(node => node.kind === 'package' && node.show_in_readme);
        assert.ok(expectedPackages.length > 0);
        assert.deepEqual([...packageLinks].sort(), expectedPackages.map(node => node.url).sort());
        await firstPackage.focus();

        assert.equal(await firstPackage.evaluate(element => document.activeElement === element), true);
        results.push({ check: 'keyboard details, focus return, package links', packageLinks });

        await page.setViewportSize({ width: 390, height: 600 });
        await page.evaluate(async () => {
            window.scrollTo(0, 0);
            await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        });
        await page.screenshot({ path: join(output, 'browser-mobile-viewport.png') });
        await page.screenshot({ path: join(output, 'browser-mobile.png'), fullPage: true });

        assert.equal(await page.locator('.semantic-profile details').getAttribute('open'), '');
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
        await page.locator('.footer').scrollIntoViewIfNeeded();

        assert.equal(await page.locator('.footer').isVisible(), true);
        assert.equal(await page.locator('[data-layer="capabilities"]').isVisible(), true);
        results.push({ check: 'mobile semantic reflow, no horizontal overflow, footer and all layers accessible' });

        // Focus first because a locator click would scroll the header out of the overlap regression.
        await projectButton.focus();
        await page.evaluate(async () => {
            window.scrollTo(0, 0);
            await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        });

        await page.keyboard.press('Enter');
        assert.equal(await page.evaluate(() => window.scrollY), 0);
        assert.equal(await page.locator('#detail-panel').getAttribute('aria-hidden'), 'false');
        const inspectorClose = page.locator('#detail-close');
        await page.waitForFunction(() => document.querySelector('#detail-panel').getAnimations()
            .every(animation => animation.playState !== 'running'));
        await page.screenshot({ path: join(output, 'browser-mobile-inspector.png') });

        const closeIsUnobscured = await inspectorClose.evaluate(element => {
            const bounds = element.getBoundingClientRect();
            const hit = document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);

            return hit?.closest('button') === element;
        });

        assert.equal(closeIsUnobscured, true);
        await inspectorClose.click();
        assert.equal(await page.locator('#detail-panel').getAttribute('aria-hidden'), 'true');
        results.push({ check: 'mobile inspector close stays above header chrome and accepts pointer input' });

        await page.setViewportSize({ width: 1500, height: 1100 });
        await page.locator('[data-layer="systems"]').click();

        const touch = await page.evaluate(async () => {
            const state = await (await fetch('./profile-state.json')).json();
            const rect = document.querySelector('#stage').getBoundingClientRect();
            const node = state.nodes.find(candidate => candidate.kind === 'project');
            const fit = Math.min(rect.width / state.canvas.width, rect.height / state.canvas.height) * 1.08;
            const x = (node.x - state.canvas.width / 2) * fit + rect.width / 2;
            const y = (node.y - state.canvas.height / 2) * fit + rect.height / 2;
            return { x: rect.left + x, y: rect.top + y, name: node.label };
        });

        await page.touchscreen.tap(touch.x, touch.y);

        assert.equal(await page.locator('#detail-title').textContent(), touch.name);
        await page.locator('#detail-close').click();
        results.push({ check: 'touch tap without pointermove activates graph node', node: touch.name });

        const hoverChecks = await checkPausedHoverExit(page, touch);
        results.push({ check: 'paused and reduced-motion hover exit repaint once without simulation', hoverChecks });


        // CDP emits a real cancellation; a DOM event would bypass Chromium's pointer synthesis.
        const cdp = await context.newCDPSession(page);
        await cdp.send('Input.dispatchTouchEvent', {
            type: 'touchStart',
            touchPoints: [{ x: touch.x, y: touch.y }],
        });
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
        await cdp.detach();

        assert.equal(await page.locator('#detail-panel').getAttribute('aria-hidden'), 'true');
        results.push({ check: 'actual touch cancellation does not select a node' });
        await page.screenshot({ path: join(output, 'browser-systems.png'), fullPage: true });

        const wasmProbe = await page.evaluate(async () => {
            const module = await import('./pkg/sourcefield_wasm.js');
            await module.default();
            let rejected = false;

            try {
                new module.Simulator([10, 10], [10, 10], [], [], 1, -1, 1680);
            } catch {
                rejected = true;
            }

            const simulator = new module.Simulator([2200, 1900], [2200, 1900], [], [], 1, 2200, 1900);
            let bounded = true;

            try {
                for (let index = 0; index < 1000; index++) {
                    const positions = simulator.tick(0.05);
                    bounded &&= positions[0] >= 0 && positions[0] <= 2200
                        && positions[1] >= 0 && positions[1] <= 1900;
                }
            } finally {
                simulator.free();
            }

            return { rejected, bounded };
        });

        assert.deepEqual(wasmProbe, { rejected: true, bounded: true });
        results.push({ check: 'actual WASM negative input and custom bounds', ...wasmProbe });

        const reduced = await context.newPage();
        await reduced.emulateMedia({ reducedMotion: 'reduce' });
        await reduced.setViewportSize({ width: 1200, height: 900 });
        await reduced.goto(base);
        await reduced.waitForSelector('#profile-view svg');

        assert.equal(await reduced.locator('#motion-button').isDisabled(), true);
        assert.equal(await reduced.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).filter(animation => animation.playState === 'running').length), 0);
        results.push({ check: 'initial reduced-motion preference' });
        await reduced.close();

        const fallback = await context.newPage();
        await fallback.route('**/pkg/sourcefield_wasm.js', route => route.abort());
        await fallback.addInitScript(() => {
            const getContext = HTMLCanvasElement.prototype.getContext;
            HTMLCanvasElement.prototype.getContext = function(type, ...args) {
                return type === 'webgl2' ? null : getContext.call(this, type, ...args);
            };
        });

        await fallback.goto(base);
        await fallback.waitForFunction(() => document.querySelector('#engine-label').textContent
            .includes('JAVASCRIPT FALLBACK / CANVAS2D'));

        assert.equal(await fallback.locator('#profile-view svg').count(), 1);
        results.push({
            check: 'intentional missing WASM and unavailable WebGL fallback',
            engine: await fallback.locator('#engine-label').textContent(),
        });
        await fallback.close();

        const history = await context.newPage();
        const response = await context.request.get(new URL('profile-state.json', base).href);
        assert.ok(response.ok(), 'The current profile state must be available for the history fixture.');

        const current = await response.json();
        const hash = 'ABCDEF0123456789';
        const filename = `${hash.toLowerCase()}.json`;
        const generatedAt = '2026-09-01T00:00:00Z';
        await history.route('**/history/index.json', route => route.fulfill({
            json: { schema_version:3, states: [{ hash, generated_at: generatedAt, file: filename }] },
        }));
        await history.route(`**/history/${filename}`, route => route.fulfill({
            json: { ...current, semantic_hash: hash, generated_at: generatedAt },
        }));

        await history.goto(base);
        await history.waitForFunction(() => document.querySelector('#history-select').options.length === 2);
        await history.locator('#history-select').selectOption(hash.toLowerCase());
        await history.waitForFunction(() => document.querySelector('#state-label').textContent.toLowerCase().includes('abcd'));

        assert.equal(await history.locator('[data-layer="overview"]').isDisabled(), true);
        await history.locator('#history-select').selectOption('');
        await history.waitForFunction(() => !document.querySelector('[data-layer="overview"]').disabled);
        results.push({ check: 'historical snapshot and current-state restoration' });

        await history.unroute(`**/history/${filename}`);
        await history.route(`**/history/${filename}`, route => route.fulfill({
            json: { ...current, canvas: { width: -1, height: 1680 } },
        }));

        const countBeforeCorruptHistory = await history.locator('#node-list button').count();
        await history.locator('#history-select').selectOption(hash.toLowerCase());
        await history.waitForFunction(() => document.querySelector('#state-label').textContent
            === 'SNAPSHOT UNAVAILABLE');

        assert.equal(await history.locator('#node-list button').count(), countBeforeCorruptHistory);
        results.push({ check: 'corrupt historical canvas rejected without replacing active nodes' });
        await history.close();

        const producerDirectory = process.env.SOURCEFIELD_TEST_HISTORY_OUTPUT;

        if (producerDirectory) {
            // Consume bytes emitted by the Rust producer, rather than recreating its naming rules.
            const indexBytes = await readFile(join(producerDirectory, 'index.json'));
            const index = JSON.parse(indexBytes);
            const entry = index.states.find(item => /[A-F]/.test(item.hash));
            assert.ok(entry, 'The producer fixture must exercise uppercase alphabetic hash digits.');
            assert.equal(entry.file, `${entry.hash.toLowerCase()}.json`);
            const archiveBytes = await readFile(join(producerDirectory, entry.file));
            const generated = await context.newPage();
            await generated.route('**/history/index.json', route => route.fulfill({
                body: indexBytes, contentType: 'application/json',
            }));
            await generated.route(`**/history/${entry.file}`, route => route.fulfill({
                body: archiveBytes, contentType: 'application/json',
            }));

            await generated.goto(base);
            await generated.waitForFunction(() => document.querySelector('#history-select').options.length > 1);
            await generated.locator('#history-select').selectOption(entry.hash.toLowerCase());
            await generated.waitForFunction(hash => document.querySelector('#state-label').textContent
                .toLowerCase().includes(hash.slice(0, 4).toLowerCase()), entry.hash);

            assert.equal(await generated.locator('[data-layer="overview"]').isDisabled(), true);
            assert.equal(await generated.locator('#node-list button').count(), JSON.parse(archiveBytes).nodes.filter(node => node.kind !== 'domain').length);
            await generated.close();
            results.push({ check: 'real Rust producer history archive selected', hash: entry.hash });
        }

        assert.deepEqual(errors, []);
        results.push({ check: 'normal load has no browser errors or CSP violations', errors });
        assert.equal(results.length, producerDirectory ? 16 : 15);

        return { results };
    } finally {
        await context.close();
    }
}

/**
 * Verify Chromium canvas invalidation without exposing production runtime internals.
 * @param {import('playwright').Page} page Browser with a paused Systems layer.
 * @param {{x: number, y: number}} point Visible project coordinates in the current viewport.
 * @returns {Promise<object[]>} Observed repaint and simulation counts for each motion preference.
 */
async function checkPausedHoverExit(page, point) {
    await page.evaluate(async () => {
        const module = await import('./pkg/sourcefield_wasm.js');
        const clear = CanvasRenderingContext2D.prototype.clearRect;
        const advance = module.Simulator.prototype.advance;
        window.hoverProbe = { paints: 0, advances: 0 };
        CanvasRenderingContext2D.prototype.clearRect = function(...args) {
            if (this.canvas.id === 'label-canvas') window.hoverProbe.paints++;

            return clear.apply(this, args);
        };
        module.Simulator.prototype.advance = function(...args) {
            window.hoverProbe.advances++;

            return advance.apply(this, args);
        };
        window.restoreHoverProbe = () => {
            CanvasRenderingContext2D.prototype.clearRect = clear;
            module.Simulator.prototype.advance = advance;
            delete window.hoverProbe;
            delete window.restoreHoverProbe;
        };
    });

    const checks = [];
    const settle = () => page.evaluate(() => new Promise(resolve =>
        requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(resolve)))));

    try {
        for (const reducedMotion of ['no-preference', 'reduce']) {
            // Arrange
            await page.emulateMedia({ reducedMotion });
            await page.mouse.move(1, 1);
            await settle();
            await page.mouse.move(point.x, point.y);
            await settle();
            const before = await page.evaluate(() => ({ ...window.hoverProbe }));

            // Act
            await page.mouse.move(1, 1);
            await settle();
            const after = await page.evaluate(() => ({ ...window.hoverProbe }));

            // Assert
            assert.equal(after.paints - before.paints, 1, `${reducedMotion}: hover exit needs one repaint`);
            assert.equal(after.advances - before.advances, 0, `${reducedMotion}: simulation must stay paused`);
            assert.equal(await page.locator('#motion-button').getAttribute('aria-pressed'), 'true');
            checks.push({
                reducedMotion,
                paints: after.paints - before.paints,
                advances: after.advances - before.advances,
            });
        }
    } finally {
        await page.evaluate(() => window.restoreHoverProbe());
        await page.emulateMedia({ reducedMotion: 'no-preference' });
    }

    return checks;
}
