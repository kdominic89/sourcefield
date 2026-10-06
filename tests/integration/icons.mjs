import assert from 'node:assert/strict';
import { join } from 'node:path';

/** Compare numeric SVG attributes without depending on floating-point text formatting. */
function near(actual, expected, message) {
    assert.ok(Math.abs(Number(actual) - expected) < 0.002, `${message}: ${actual} != ${expected}`);
}

/** Decode renderer-owned absolute path commands for coordinate-level artwork assertions. */
function pathTokens(path) {
    return path.match(/[MLHVQCAZ]|[-+]?(?:\d*\.?\d+)(?:[eE][-+]?\d+)?/g)
        .map(token => /^[MLHVQCAZ]$/.test(token) ? token : Number(token));
}

/** Compare each command and coordinate, retaining the distinction between arcs and lines. */
function assertPath(actual, expected) {
    const tokens = pathTokens(actual);
    assert.equal(tokens.length, expected.length);

    tokens.forEach((token, index) => {
        if (typeof expected[index] === 'number') {
            near(token, expected[index], `path coordinate ${index}`);
        } else {
            assert.equal(token, expected[index]);
        }
    });
}

/** Verify accepted geometry independently of the implementation's typed catalog. */
function assertApprovedArtwork(icons, theme) {
    const colors = theme === 'dark'
        ? { mint: '#4de7c2', blue: '#7acfff', purple: '#a898ff', recess: '#090f1c', surface: '#0d1425' }
        : { mint: '#006c59', blue: '#17628e', purple: '#6243b5', recess: '#e2e8f3', surface: '#ffffff' };

    for (const icon of icons) {
        const elements = icon.elements;

        if (icon.key === 'builtin:sourcefield') {
            const points = [[-2, 2, 4.1, 'mint'], [-18, -12, 3, 'blue'], [13, -18, 3.2, 'purple'],
                [20, 8, 3, 'blue'], [-12, 20, 2.8, 'mint']];
            const links = [[0, 1], [0, 2], [0, 3], [0, 4], [1, 2], [2, 3]];
            assert.equal(elements.length, 14);

            links.forEach(([from, to], index) => {
                const a = points[from];
                const b = points[to];
                const dx = b[0] - a[0];
                const dy = b[1] - a[1];
                const distance = Math.hypot(dx, dy);
                const rounded = value => Math.round(value * 1000) / 1000;
                assertPath(elements[index].attributes.d, ['M', rounded(a[0] + dx / distance * a[2]),
                    rounded(a[1] + dy / distance * a[2]), 'L', rounded(b[0] - dx / distance * b[2]),
                    rounded(b[1] - dy / distance * b[2])]);
                near(elements[index].attributes['stroke-width'], 1.25, 'network link width');
            });

            const circles = elements.slice(6);
            let cursor = 0;

            points.forEach(([x, y, radius, paint], index) => {
                const circle = circles[cursor++];
                assert.equal(circle.tag, 'circle');
                near(circle.attributes.cx, x, 'network node x');
                near(circle.attributes.cy, y, 'network node y');
                near(circle.attributes.r, radius, 'network node radius');
                assert.equal(circle.attributes.fill, colors.surface);
                assert.equal(circle.attributes.stroke, colors[paint]);

                if ([0, 2, 4].includes(index)) {
                    const dot = circles[cursor++];
                    near(dot.attributes.cx, x, 'network signal x');
                    near(dot.attributes.cy, y, 'network signal y');
                    near(dot.attributes.r, radius * 0.45, 'network signal radius');
                    assert.equal(dot.attributes.fill, colors[paint]);
                }
            });
        } else if (icon.key === 'builtin:database-safe') {
            assert.equal(elements.length, 13);
            const body = elements[0].attributes;
            near(body.x, -23, 'safe body x');
            near(body.y, -21, 'safe body y');
            near(body.width, 31, 'safe body width');
            near(body.height, 42, 'safe body height');
            near(body.rx, 3, 'safe body corner');
            assert.equal(body.fill, colors.recess);
            assertPath(elements[1].attributes.d,
                ['M', -17.7, -10, 'V', 8.9, 'A', 10.2, 4.2, 0, 0, 0, 2.7, 8.9, 'V', -10]);
            const lid = elements[2].attributes;
            assert.equal(elements[2].tag, 'ellipse');
            near(lid.cx, -7.5, 'cylinder lid x');
            near(lid.cy, -10, 'cylinder lid y');
            near(lid.rx, 10.2, 'cylinder lid width');
            near(lid.ry, 4.2, 'cylinder lid depth');

            [-3.95, 2.35, 8.65].forEach((y, index) => {
                const dot = elements[5 + index * 2].attributes;
                near(dot.cx, -14.9, 'cylinder dot x');
                near(dot.cy, y, 'cylinder dot y');
                near(dot.r, 0.53, 'cylinder dot radius');
            });

            assertPath(elements[11].attributes.d, ['M', 8, -18, 'L', 23, -22, 'V', 22, 'L', 8, 18, 'Z']);
            assertPath(elements[12].attributes.d, ['M', 18, -3, 'V', 4]);
            assert.equal(elements[11].attributes.fill, colors.surface);
        }
    }
}

/** Read native and application SVG geometry through one DOM representation. */
function readIconMarkup(selector) {
    const svg = document.querySelector(selector);
    const icons = [...svg.querySelectorAll('[data-icon]')].map(icon => {
        const clip = icon.parentElement.getAttribute('clip-path');
        const id = /^url\(#([\w-]+)\)$/.exec(clip)?.[1];
        const circle = id ? svg.querySelector(`#${CSS.escape(id)} circle`) : null;
        const transform = /^scale\(([^)]+)\)$/.exec(icon.getAttribute('transform'));

        return {
            key: icon.dataset.icon,
            node: icon.closest('[data-node-id]').dataset.nodeId,
            scale: transform ? Number(transform[1]) : null,
            clip: id,
            clipRadius: circle ? Number(circle.getAttribute('r')) : null,
            elements: [...icon.children].map(element => ({
                tag: element.localName,
                attributes: Object.fromEntries([...element.attributes]
                    .filter(attribute => attribute.name !== 'class')
                    .map(attribute => [attribute.name, attribute.value])),
            })),
        };
    });

    return {
        icons,
        signals: svg.querySelectorAll('.icon-signal').length,
        inlineStyles: svg.querySelectorAll('[style], style').length,
        forbidden: svg.querySelectorAll('use, script, image, foreignObject, animate, set').length,
    };
}

/** Host untouched, trusted generator output with an explicit favicon and independent native CSS. */
async function loadNativePreview(page, base, flavor) {
    const response = await page.request.get(new URL(`sourcefield.${flavor}.svg`, base).href);
    assert.equal(response.ok(), true, `Missing native ${flavor} SVG`);
    const source = await response.text();
    const url = new URL(`icon-preview-${flavor}.html`, base).href;
    await page.route(url, route => route.fulfill({
        contentType: 'text/html',
        body: '<!doctype html><html><head><meta charset="utf-8">'
            + '<link rel="icon" href="data:,"></head><body>' + source + '</body></html>',
    }));
    await page.goto(url);
    await page.waitForSelector('svg [data-icon]');
}

/** Compare live sanitization with untouched native SVG in a separate preview document. */
async function inspectIcons(page, flavor) {
    const nativePage = await page.context().newPage();

    try {
        await loadNativePreview(nativePage, page.url(), flavor);
        const native = await nativePage.evaluate(readIconMarkup, 'svg');
        const live = await page.evaluate(readIconMarkup, '#profile-view svg');

        return {
            live: live.icons, native: native.icons, staticSignals: native.signals,
            inlineStyles: live.inlineStyles, forbidden: live.forbidden,
        };
    } finally {
        await nativePage.close();
    }
}

/** Probe exact phase endpoints through the browser's real CSS Animation objects. */
async function signalTiming(page) {
    return page.evaluate(() => {
        const svg = document.querySelector('#profile-view svg');
        const animations = svg.getAnimations({ subtree: true })
            .filter(animation => animation.animationName === 'icon-signal');

        return {
            markers: svg.querySelectorAll('.icon-signal').length,
            signals: animations.map(animation => {
                const original = animation.currentTime;
                const timing = animation.effect.getTiming();
                const target = animation.effect.target;
                const phase = [0, 1, 2].find(value => target.classList.contains(`icon-signal-phase-${value}`));
                const start = timing.delay + 10800;
                const opacities = [];

                try {
                    for (const offset of [0, 2430, 5400]) {
                        animation.currentTime = start + offset;
                        opacities.push(Number(getComputedStyle(target).opacity));
                    }
                } finally {
                    animation.currentTime = original;
                }

                return { phase, duration: timing.duration, delay: timing.delay,
                    opacities, playState: animation.playState };
            }),
        };
    });
}

/** Require every rendered signal to retain its fixed cycle, stagger, and full opacity range. */
function assertTiming(result, expectedCount, playState = 'running') {
    assert.equal(result.markers, expectedCount);
    assert.equal(result.signals.length, expectedCount);

    for (const signal of result.signals) {
        assert.ok([0, 1, 2].includes(signal.phase));
        assert.equal(signal.duration, 5400);
        assert.equal(signal.delay, signal.phase === 0 ? 0 : -1800 * signal.phase);
        assert.equal(signal.playState, playState);
        signal.opacities.forEach((opacity, index) => near(opacity, index === 1 ? 1 : 0.35, 'signal opacity'));
    }
}

/** Require catalog selectors, unique local clips, exact fitting, and approved artwork in each theme. */
function assertIcons(snapshot, state, theme) {
    const expected = state.nodes.filter(node => node.kind === 'project' && node.show_in_readme && node.icon);
    assert.deepEqual(snapshot.live, snapshot.native);
    assert.equal(snapshot.live.length, expected.length);
    assert.equal(new Set(snapshot.live.map(icon => icon.clip)).size, expected.length);
    assert.equal(snapshot.inlineStyles, 0);
    assert.equal(snapshot.forbidden, 0);

    for (const node of expected) {
        const icon = snapshot.live.find(candidate => candidate.node === node.id);
        assert.ok(icon, `missing icon for ${node.id}`);
        assert.equal(icon.key, node.icon);
        assert.match(icon.clip, /^icon-clip-[a-f\d]+$/i);
        const authoredRadius = { 'builtin:sourcefield': 31, 'builtin:database-safe': 35 }[node.icon]
            ?? state.icons[node.icon].radius;
        const innerRadius = Math.max(1, node.radius / (state.canvas.width / 1800) - 16);
        near(icon.clipRadius, innerRadius, 'circular clip stays inside node');
        near(icon.scale, Math.min(1, innerRadius / authoredRadius), 'shrink-only icon fit');

        if (node.icon === 'browser-probe' || node.icon.endsWith('/browser-probe')) {
            assert.ok(icon.scale < 1, 'custom fixture must exercise downscaling');
            assert.deepEqual(icon.elements.map(element => element.tag), ['rect', 'circle', 'ellipse', 'path']);
            const rect = icon.elements[0].attributes;
            near(rect.x, -48, 'custom rectangle x');
            near(rect.width, 96, 'custom rectangle width');
            assert.ok(Math.hypot(48, 48) * icon.scale > icon.clipRadius, 'custom corners must exercise clipping');
            assert.equal(rect.fill, theme === 'dark' ? '#090f1c' : '#e2e8f3');
            assertPath(icon.elements[3].attributes.d, ['M', -20, 0, 'L', 20, 0]);
        }
    }

    assertApprovedArtwork(snapshot.live, theme);
}

/** Simulate visibility events after corrupting real effects, requiring the listener to restore timing. */
async function visibilityRoundTrip(page, bubbles = true) {
    return page.evaluate(bubbles => {
        const svg = document.querySelector('#profile-view svg');
        const signals = svg.getAnimations({ subtree: true })
            .filter(animation => animation.animationName === 'icon-signal');
        const before = signals.map(animation => animation.currentTime);

        try {
            for (const animation of signals) {
                animation.effect.updateTiming({ duration: 7777, delay: 137 });
            }

            Object.defineProperty(document, 'hidden', { configurable: true, value: true });
            document.dispatchEvent(new Event('visibilitychange', { bubbles }));
            const hidden = signals.map(animation => ({
                duration: animation.effect.getTiming().duration,
                delay: animation.effect.getTiming().delay,
                playState: animation.playState,
            }));
            Object.defineProperty(document, 'hidden', { configurable: true, value: false });
            document.dispatchEvent(new Event('visibilitychange', { bubbles }));

            return { before, hidden, after: signals.map(animation => animation.currentTime) };
        } finally {
            delete document.hidden;
        }
    }, bubbles);
}

/**
 * Exercise native icon output in the actual WASM-backed application and independent README-sized SVGs.
 * @param {{browser: object, base: string, output: string, requireIcons?: boolean}} options Existing browser and site.
 * @returns {Promise<object>} Geometry, theme, signal lifecycle, sanitization, and image evidence.
 */
export async function runIconChecks({ browser, base, output, requireIcons = false }) {
    const context = await browser.newContext({ viewport: { width: 1500, height: 1100 }, colorScheme: 'dark' });

    try {
        const response = await context.request.get(new URL('profile-state.json', base).href);
        assert.equal(response.ok(), true);
        const state = await response.json();
        const selected = state.nodes.filter(node => node.kind === 'project' && node.show_in_readme && node.icon);

        if (requireIcons) {
            for (const key of ['builtin:sourcefield', 'builtin:database-safe', 'browser-probe']) {
                assert.ok(selected.some(node => node.icon === key || node.icon.endsWith(`/${key}`)),
                    `required browser icon fixture is missing ${key}`);
            }
        }

        if (selected.length === 0) return { legacy: true, requiredFixture: false };

        const expectedSignals = selected.reduce((count, node) => count + (node.icon === 'builtin:sourcefield' ? 3
            : node.icon === 'builtin:database-safe' ? 0
                : state.icons[node.icon].elements.filter(element => element.motion).length), 0);
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        page.on('console', message => {
            if (message.type() === 'error') errors.push(message.text());
        });
        await page.addInitScript(() => {
            globalThis.iconPolicyViolations = [];
            document.addEventListener('securitypolicyviolation', event => {
                globalThis.iconPolicyViolations.push(event.violatedDirective);
            });
        });
        await page.goto(base);
        await page.waitForFunction(() => document.querySelector('#engine-label').textContent.includes('RUST/WASM'));
        await page.waitForSelector('#profile-view [data-icon]');
        const dark = await inspectIcons(page, 'dark');
        const initial = await signalTiming(page);
        const runningVisibility = await visibilityRoundTrip(page);
        const restoredRunningVisibility = await signalTiming(page);
        const directRunningVisibility = await visibilityRoundTrip(page, false);
        const restoredDirectRunningVisibility = await signalTiming(page);
        await page.locator('#motion-button').click();
        const paused = await signalTiming(page);
        const pausedVisibility = await visibilityRoundTrip(page);
        const restoredPausedVisibility = await signalTiming(page);
        const directPausedVisibility = await visibilityRoundTrip(page, false);
        const restoredDirectPausedVisibility = await signalTiming(page);
        await page.locator('[data-layer="systems"]').click();
        await page.locator('[data-layer="overview"]').click();
        const restoredLayer = await signalTiming(page);
        const oldSvg = await page.locator('#profile-view svg').elementHandle();
        await page.emulateMedia({ colorScheme: 'light' });
        await oldSvg.waitForElementState('hidden');
        await oldSvg.dispose();
        await page.waitForSelector('#profile-view [data-icon]');
        const light = await inspectIcons(page, 'light');
        const themedPause = await signalTiming(page);
        await page.locator('#motion-button').click();
        const resumed = await signalTiming(page);
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.waitForFunction(() => document.querySelector('#motion-button').disabled);
        const reduced = await signalTiming(page);
        const initialPolicyViolations = await page.evaluate(() => globalThis.iconPolicyViolations);
        await page.reload();
        await page.waitForSelector('#profile-view [data-icon]');
        const initialReduced = await signalTiming(page);
        await page.emulateMedia({ reducedMotion: 'no-preference' });
        await page.waitForFunction(() => !document.querySelector('#motion-button').disabled);
        const restoredMotion = await signalTiming(page);
        const policyViolations = initialPolicyViolations.concat(
            await page.evaluate(() => globalThis.iconPolicyViolations),
        );
        const staticSnapshot = await inspectIcons(page, 'static');
        const readme = await context.newPage();
        readme.on('pageerror', error => errors.push(error.message));
        readme.on('console', message => {
            if (message.type() === 'error') errors.push(message.text());
        });
        await readme.setViewportSize({ width: 900, height: 900 });
        const images = [];

        for (const flavor of ['dark', 'light', 'static']) {
            await loadNativePreview(readme, base, flavor);
            const native = await readme.evaluate(() => {
                const svg = document.querySelector('svg');
                const viewBox = svg.viewBox.baseVal;
                svg.setAttribute('width', '900');
                svg.setAttribute('height', String(900 * viewBox.height / viewBox.width));
                const all = svg.getAnimations({ subtree: true });
                all.forEach(animation => animation.pause());

                return {
                    animations: all.filter(animation => animation.animationName === 'icon-signal').length,
                    width: svg.getBoundingClientRect().width,
                };
            });
            const filename = `icons-readme-${flavor}.png`;
            await readme.locator('svg').screenshot({ path: join(output, filename) });
            await readme.emulateMedia({ reducedMotion: 'reduce' });
            const reducedAnimations = await readme.evaluate(() => document.querySelector('svg')
                .getAnimations({ subtree: true })
                .filter(animation => animation.animationName === 'icon-signal').length);
            await readme.emulateMedia({ reducedMotion: 'no-preference' });
            images.push({ flavor, filename, ...native, reducedAnimations });
        }

        assertIcons(dark, state, 'dark');
        assertIcons(light, state, 'light');
        assert.deepEqual(staticSnapshot.native, dark.native);
        assert.equal(staticSnapshot.staticSignals, 0);
        assertTiming(initial, expectedSignals);
        assertTiming(paused, expectedSignals, 'paused');

        for (const [visibility, restored, playState] of [
            [runningVisibility, restoredRunningVisibility, 'running'],
            [directRunningVisibility, restoredDirectRunningVisibility, 'running'],
            [pausedVisibility, restoredPausedVisibility, 'paused'],
            [directPausedVisibility, restoredDirectPausedVisibility, 'paused'],
        ]) {
            assert.deepEqual(visibility.hidden,
                Array.from({ length: expectedSignals }, () => ({ duration: 7777, delay: 137, playState })));
            assertTiming(restored, expectedSignals, playState);

            if (playState === 'paused') {
                assert.deepEqual(visibility.before, visibility.after);
            }
        }

        assertTiming(restoredLayer, expectedSignals, 'paused');
        assertTiming(themedPause, expectedSignals, 'paused');
        assertTiming(resumed, expectedSignals);
        assert.equal(reduced.signals.length, 0);
        assert.equal(initialReduced.signals.length, 0);
        assertTiming(restoredMotion, expectedSignals);
        assert.deepEqual(images.map(image => image.animations), [expectedSignals, expectedSignals, 0]);
        assert.ok(images.every(image => image.width === 900 && image.reducedAnimations === 0));
        assert.deepEqual(policyViolations, []);
        assert.deepEqual(errors, []);

        return { requiredFixture: requireIcons, icons: dark.live, initial, paused, restoredLayer, themedPause,
            resumed, reduced, initialReduced, restoredMotion, images, policyViolations, errors,
            syntheticVisibility: { running: runningVisibility, restoredRunning: restoredRunningVisibility,
                paused: pausedVisibility, restoredPaused: restoredPausedVisibility,
                directRunning: directRunningVisibility, restoredDirectRunning: restoredDirectRunningVisibility,
                directPaused: directPausedVisibility, restoredDirectPaused: restoredDirectPausedVisibility },
            engine: 'RUST/WASM', nativeMarkupPreserved: true, staticGeometryPreserved: true };
    } finally {
        await context.close();
    }
}
