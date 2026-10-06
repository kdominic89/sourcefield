import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

/** Execute the production timing boundary without booting browser I/O. */
async function configure(animations) {
    const source = await readFile(new URL('../runtime/app.js', import.meta.url), 'utf8');
    const start = source.indexOf('function configureProfileAnimations(');
    const end = source.indexOf('/** Load the canonical renderer', start);
    const context = vm.createContext({ svg: { getAnimations: () => animations } });

    vm.runInContext(`${source.slice(start, end)}\nconfigureProfileAnimations(svg, 32);`, context);
}

for (const phase of [0, 1, 2]) {
    test(`icon signal phase ${phase} uses fixed numeric timing`, async () => {
        const timings = [];
        const animation = {
            animationName: 'icon-signal',
            effect: {
                target: { classList: { contains: value => value === `icon-signal-phase-${phase}` } },
                updateTiming: timing => timings.push({ ...timing }),
            },
        };

        await configure([animation]);

        assert.deepEqual(timings, [{ duration: 5400, delay: [0, -1800, -3600][phase] }]);
    });
}

test('untrusted phase-like classes and data never supply animation timing', async () => {
    const timings = [];
    const names = ['icon-signal-phase-3', 'icon-signal-phase--1', 'icon-signal-phase-Infinity',
        'icon-signal-phase-url(https://example.com)', 'icon-signal-phase-1.5', ''];
    const animations = names.map(name => ({
        animationName: 'icon-signal',
        effect: {
            target: {
                dataset: { signalDelay: '-9000', iconPhase: 'url(https://example.com)' },
                classList: { contains: value => value === name },
            },
            updateTiming: timing => timings.push({ ...timing }),
        },
    }));

    await configure(animations);

    assert.equal(timings.length, names.length);
    assert.ok(timings.every(timing => timing.duration === 5400 && timing.delay === 0));
});

test('recreated icon effects regain phases independently of unrelated animation names', async () => {
    const timings = [];
    const create = animationName => ({
        animationName,
        effect: {
            target: { classList: { contains: value => value === 'icon-signal-phase-2' } },
            updateTiming: timing => timings.push({ ...timing }),
        },
    });

    await configure([create('icon-signal')]);
    await configure([create('icon-signal'), create('unrecognized')]);

    assert.deepEqual(timings, [
        { duration: 5400, delay: -3600 },
        { duration: 5400, delay: -3600 },
    ]);
});

test('external CSS preserves icon signal timing and reduced-motion and pause rules', async () => {
    const stylesheet = await readFile(new URL('../runtime/app.css', import.meta.url), 'utf8');

    const reduced = stylesheet.slice(stylesheet.lastIndexOf('@media (prefers-reduced-motion: reduce)'));

    assert.match(stylesheet, /\.icon-signal\s*\{\s*animation: icon-signal 5\.4s ease-in-out infinite;/);
    assert.match(stylesheet, /\.icon-signal-phase-1\s*\{\s*animation-delay: -1\.8s;/);
    assert.match(stylesheet, /\.icon-signal-phase-2\s*\{\s*animation-delay: -3\.6s;/);
    assert.match(stylesheet, /@keyframes icon-signal\s*\{\s*0%, 100%\s*\{\s*opacity: \.35;/);
    assert.match(reduced, /#profile-view \.icon-signal\s*\{\s*animation: none !important;/);
    assert.match(stylesheet, /\.motion-paused \*[^}]+animation-play-state: paused !important;/);
});
