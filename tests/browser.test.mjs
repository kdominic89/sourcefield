import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import { Simulator } from '../runtime/simulation-fallback.js';

/** Load browser helpers in a minimal host without starting I/O or animation. */
async function browserHarness({ reducedMotion = false } = {}) {
    const makeElement = () => ({
        children: [],
        textContent: '',
        dataset: {},
        append(...children) { this.children.push(...children); },
        replaceChildren(...children) { this.children = children; },
    });
    const handlers = new Map();
    const elements = new Map();
    const element = selector => {
        if (!elements.has(selector)) {
            elements.set(selector, {
                ...makeElement(),
                addEventListener: (event, handler) => handlers.set(`${selector}:${event}`, handler),
                classList: { add() {}, remove() {}, toggle() {} },
                setAttribute() {}, focus() {}, contains() {return false;}, isConnected:true,
                getBoundingClientRect: () => ({ left: 20, top: 30 }),
                setPointerCapture() {}, releasePointerCapture() {},
                querySelector: () => null,
            });
        }

        return elements.get(selector);
    };
    const source = (await readFile(new URL('../runtime/app.js', import.meta.url), 'utf8'))
        .replace(/^import .*;\n/m, '')
        .replace(/boot\(\)\.catch\([\s\S]*$/, '');
    const context = vm.createContext({
        window: {
            matchMedia: query => ({
                matches: query === '(prefers-reduced-motion: reduce)' && reducedMotion,
                addEventListener: (event, handler) => handlers.set(`${query}:${event}`, handler),
            }),
            addEventListener: (event, handler) => handlers.set(`window:${event}`, handler),
        },
        document: Object.assign(new EventTarget(), {
            querySelector: element, querySelectorAll: () => [],
            createElement: makeElement, body: element('body'), documentElement: element('html'),
        }),
        performance: { now: () => 0 }, URL,
    });
    vm.runInContext(source, context);
    vm.runInContext('bindEvents()', context);

    return { context, handlers, elements, evaluate: code => vm.runInContext(code, context) };
}

test('fallback motion remains deterministic and bounded', () => {
    const anchors = [0, 0, 2200, 1900];
    const left = new Simulator(anchors, anchors, [0, 1], [1], 42, 2200, 1900);
    const right = new Simulator(anchors, anchors, [0, 1], [1], 42, 2200, 1900);
    let bounded = true;

    for (let iteration = 0; iteration < 1000; iteration++) {
        left.advance(.05);
        right.advance(.05);
        bounded &&= left.positionData.every((value, index) => value >= 0 && value <= (index % 2 ? 1900 : 2200));
    }

    assert.deepEqual(left.positions(), right.positions());
    assert.ok(bounded);
});

test('fallback reset restores anchors after motion', () => {
    const anchors = [10,20];
    const simulator = new Simulator(anchors,anchors,[],[]);
    simulator.advance(.05);

    const reset = simulator.reset();

    assert.deepEqual([...reset], anchors);
    assert.equal(simulator.elapsed,0);
});

test('fallback rejects malformed inputs and ignores nonfinite time', () => {
    const anchors = [100, 100];
    const simulator = new Simulator(anchors, anchors, [], []);

    const unchanged = simulator.tick(Number.NaN);

    assert.deepEqual([...unchanged], anchors);
    assert.throws(() => new Simulator([NaN, 0], anchors, [], []), RangeError);
    assert.throws(() => new Simulator(anchors, anchors, [0, 1], [1]), RangeError);
    assert.throws(() => new Simulator(anchors, anchors, [], [], 1, -1, 100), RangeError);
    assert.throws(() => new Simulator(anchors, [0], [], []), RangeError);
});

test('a tap updates its coordinates without any pointermove', async () => {
    const harness = await browserHarness();
    harness.evaluate(`app.layer = 'systems'; app.size = {width:1800,height:1680,fit:1};
        app.nodes = []; app.positions = [];`);
    const event = { clientX: 220, clientY: 330, pointerId: 1, target: { closest: () => null } };

    harness.handlers.get('#stage:pointerdown')(event);
    const x = harness.evaluate('app.pointer.x');
    const y = harness.evaluate('app.pointer.y');
    harness.handlers.get('#stage:pointerup')(event);

    assert.equal(x, 200);
    assert.equal(y, 300);
    assert.equal(harness.evaluate('app.pointer.down'), false);
});

test('canceled gestures and panel controls cannot select a node', async () => {
    const harness = await browserHarness();
    harness.evaluate("app.layer = 'systems'");
    const event = { clientX: 100, clientY: 100, pointerId: 1, target: { closest: () => null } };

    harness.handlers.get('#stage:pointerdown')(event);
    harness.handlers.get('#stage:pointercancel')(event);
    harness.handlers.get('#stage:pointerup')(event);
    harness.handlers.get('#stage:pointerdown')({ ...event, target: { closest: () => ({}) } });

    assert.equal(harness.evaluate('app.pointer.down'), false);
    assert.equal(harness.evaluate('app.selected'), null);
});

test('reduced motion overrides resume and unsafe public links are rejected', async () => {
    const harness = await browserHarness();

    harness.evaluate('app.running = true; app.reducedMotion = true; updateMotionButton()');

    assert.equal(harness.evaluate('app.running'), false);
    assert.equal(harness.evaluate("safePublicUrl('https://www.nuget.org/packages/example')"), true);
    assert.equal(harness.evaluate("safePublicUrl('javascript:alert(1)')"), false);
    assert.equal(harness.evaluate("safePublicUrl('https://user:password@example.com')"), false);
});

test('snapshot validation rejects corrupt geometry before active-state mutation', async () => {
    const harness = await browserHarness();
    harness.evaluate(`globalThis.validSnapshot = {
        nodes: [{id:'project:sample',label:'Sample',x:10,y:20}], edges: [],
        schema_version:3, canvas: {width:1800,height:1680}, profile: {variant:"personal"}, stats: {}, mode:'preview',
        semantic_hash:'0123456789abcdef'
    }`);

    const valid = harness.evaluate('validateState(validSnapshot).nodes.length');

    assert.equal(valid, 1);
    assert.throws(() => harness.evaluate('validateState({...validSnapshot, canvas:{width:NaN,height:1680}})'));
    assert.throws(() => harness.evaluate(
        'validateState({...validSnapshot, nodes:[...validSnapshot.nodes,...validSnapshot.nodes]})'));
    assert.throws(() => harness.evaluate(
        "validateState({...validSnapshot, edges:[{from:'missing',to:'project:sample',weight:1}]})"));
    assert.equal(harness.evaluate('app.state'), null);
});


for (const value of [0, null, undefined]) {
    test(`private metric preserves availability: ${value}`, async () => {
        const harness = await browserHarness();
        harness.context.metric = value;
        harness.evaluate("app.state = {stats:{private_repository_count:metric},mode:'live'}");

        harness.evaluate('renderMetrics()');
        const collected = harness.elements.get('#metrics').children;
        const privateMetric = collected.find(item => item.children[0].includes('TOKEN-VISIBLE OWNED PRIVATE'));

        assert.equal(Boolean(privateMetric), value === 0);
        assert.equal(collected.length, value === 0 ? 5 : 4);
        if (privateMetric) assert.equal(privateMetric.children[1].textContent, '0');
    });
}

for (const reducedMotion of [false,true]) {
    test(`keyboard pause respects reduced motion: ${reducedMotion}`, async () => {
        const harness = await browserHarness({reducedMotion});
        const event = {key:' ',code:'Space',target:harness.elements.get('body'),preventDefault() {}};

        harness.handlers.get('window:keydown')(event);

        assert.equal(harness.evaluate('app.running'), false);
        assert.equal(harness.evaluate('app.userPaused'), !reducedMotion);
    });
}

test('profile animation timing preserves scan speed and safe signal phases', async () => {
    const harness = await browserHarness();
    const timings = [];
    const fixtures = [
        ['orbit', '', false], ['orbit', '', true], ['flow', '', false],
        ['signal', '-0.8', false], ['signal', '0', false],
        ['signal', 'url(https://example.com)', false], ['signal', 'Infinity', false],
        ['signal', '1', false], ['signal', '-1e309', false], ['signal', '', false],
    ];
    harness.context.svg = {
        getAnimations: () => fixtures.map(([animationName, signalDelay, scan]) => ({
            animationName,
            effect: {
                target: { dataset: { signalDelay }, classList: { contains: value => value === 'scan' && scan } },
                updateTiming: timing => timings.push({ ...timing }),
            },
        })),
    };

    harness.evaluate('configureProfileAnimations(svg, 32)');

    assert.deepEqual(timings.slice(0, 5), [
        { duration: 58000 }, { duration: 24000 }, { duration: 32000 },
        { duration: 3600, delay: -800 }, { duration: 3600, delay: 0 },
    ]);
    assert.ok(timings.slice(5).every(timing => timing.duration === 3600 && timing.delay === 0));
});

for (const { name, hidden, reducedMotion, paused } of [
    { name: 'visible running', hidden: false, reducedMotion: false, paused: false },
    { name: 'visible paused', hidden: false, reducedMotion: false, paused: true },
    { name: 'hidden', hidden: true, reducedMotion: false, paused: false },
    { name: 'reduced motion', hidden: false, reducedMotion: true, paused: false },
]) {
    test(`direct document visibility event respects ${name} state`, async () => {
        const harness = await browserHarness({ reducedMotion });
        const timing = { duration: 7777, delay: 137 };
        const svg = harness.context.document.querySelector('#profile-view svg');
        const running = !reducedMotion && !paused;
        svg.getAnimations = () => [{
            animationName: 'icon-signal',
            effect: {
                target: { classList: { contains: value => value === 'icon-signal-phase-2' } },
                updateTiming: update => Object.assign(timing, update),
            },
        }];
        harness.context.document.hidden = hidden;
        harness.context.initialVisibility = { running, paused };
        harness.evaluate(`app.state = { canvas: { motion_seconds: 32 } };
            app.running = initialVisibility.running; app.userPaused = initialVisibility.paused;`);

        harness.context.document.dispatchEvent(new Event('visibilitychange'));

        assert.deepEqual(timing, hidden || reducedMotion
            ? { duration: 7777, delay: 137 } : { duration: 5400, delay: -3600 });
        assert.equal(harness.evaluate('app.running'), running);
        assert.equal(harness.evaluate('app.userPaused'), paused);
    });
}

for (const layer of ['overview','systems']) {
 test(`frame simulation follows active layer: ${layer}`, async () => {
    const harness = await browserHarness();
    let ticks = 0;
    let renders = 0;
    let decorationMoves = 0;
    harness.context.document.querySelectorAll = () => [{
        dataset: { nodeId: 'project:sample' },
        querySelector: () => ({ setAttribute: () => { decorationMoves++; } }),
    }];
    harness.context.requestAnimationFrame = () => {};
    harness.context.simulator = { advance: () => { ticks++; }, coordinate: index => [50,60][index] };
    harness.context.renderer = { render: () => { renders++; } };
    harness.evaluate(`app.simulator = simulator; app.renderer = renderer;
        readPalette = () => ({}); buildVisualFrame = () => ({}); drawOverlay = () => {};
        app.positions = [10, 20];
        app.nodeById = new Map([['project:sample', {index:0, node:{x:5, y:5}}]]);`);

    harness.context.layer = layer;
    harness.evaluate('app.layer = layer');

    harness.evaluate('frame(16)');

    assert.equal(ticks, layer === 'overview' ? 0 : 1);
    assert.equal(renders, layer === 'overview' ? 0 : 1);
    assert.equal(decorationMoves, 0);
    assert.deepEqual([...harness.evaluate('app.positions')], layer === 'overview' ? [10,20] : [50,60]);
 });
}

test('leaving reduced motion reapplies phases to recreated animations without overriding user pause', async () => {
    for (const initiallyReduced of [false, true]) {
        const harness = await browserHarness({ reducedMotion: initiallyReduced });
        const timings = [];
        const svg = harness.context.document.querySelector('#profile-view svg');
        svg.getAnimations = () => [{
            animationName: 'signal',
            effect: {
                target: { dataset: { signalDelay: '-1.6' } },
                updateTiming: timing => timings.push({ ...timing }),
            },
        }];
        harness.evaluate('app.state = {canvas:{motion_seconds:48}}; app.userPaused = true');
        const change = harness.handlers.get('(prefers-reduced-motion: reduce):change');

        change({ matches: true });
        change({ matches: false });

        assert.deepEqual(timings, [{ duration: 3600, delay: -1600 }]);
        assert.equal(harness.evaluate('app.running'), false);
        assert.equal(harness.evaluate('app.userPaused'), true);
    }
});

test('resume and resize restore timing on newly created profile effects', async () => {
    const harness = await browserHarness();
    const timings = [];
    const svg = harness.context.document.querySelector('#profile-view svg');
    svg.getAnimations = () => [{
        animationName: 'signal',
        effect: {
            target: { dataset: { signalDelay: '-0.65' } },
            updateTiming: timing => timings.push({ ...timing }),
        },
    }];
    harness.evaluate('app.state = {canvas:{motion_seconds:32}}; app.renderer = {resize() {}}; app.running = false');

    harness.handlers.get('#motion-button:click')();
    harness.handlers.get('window:resize')();

    assert.deepEqual(timings, [
        { duration: 3600, delay: -650 },
        { duration: 3600, delay: -650 },
    ]);
    assert.equal(harness.evaluate('app.running'), true);
});


test('fallback advance retains storage and matches snapshot stepping', () => {
    const anchors = [100, 100, 200, 100];
    const buffered = new Simulator(anchors, anchors, [0, 1], [.8], 42);
    const snapshots = new Simulator(anchors, anchors, [0, 1], [.8], 42);
    const positions = buffered.positionData;
    const forces = buffered.forceData;

    for (let frame = 0; frame < 1000; frame++) {
        buffered.advance(.016);
        snapshots.tick(.016);
    }

    assert.strictEqual(buffered.positionData, positions);
    assert.strictEqual(buffered.forceData, forces);
    assert.deepEqual(buffered.positions(), snapshots.positions());
    assert.ok(Number.isNaN(buffered.coordinate(100)));
});

test('fallback rejects node and edge resource exhaustion', () => {
    const nodes = new Array(1026).fill(0);
    const pairs = new Array(8193 * 2).fill(0);
    const weights = new Array(8193).fill(.5);

    const excessiveNodes = () => new Simulator(nodes, nodes, [], []);
    const excessiveEdges = () => new Simulator([0,0], [0,0], pairs, weights);

    assert.throws(excessiveNodes, RangeError);
    assert.throws(excessiveEdges, RangeError);
});

test('organization state rejects personal data before display', async () => {
    const harness = await browserHarness();
    harness.evaluate(`globalThis.organization = {
        schema_version:3, profile:{variant:'organization'}, stats:{}, mode:'preview',
        semantic_hash:'0123456789abcdef', canvas:{width:100,height:100},
        nodes:[{id:'domain:sample',kind:'domain',scope:'organization',label:'Sample',x:50,y:50}], edges:[]
    }`);

    const valid = harness.evaluate('validateState(organization).profile.variant');
    const privateHardware = () => harness.evaluate('validateState({...organization,presentation:{hardware:[{label:"Private"}]}})');
    const privateMetrics = () => harness.evaluate('validateState({...organization,stats:{private_repository_count:0}})');
    const legacy = () => harness.evaluate('validateState({...organization,schema_version:2})');

    assert.equal(valid, 'organization');
    assert.throws(privateHardware, /personal content/);
    assert.throws(privateMetrics, /personal content/);
    assert.throws(legacy, /Invalid state/);
});

test('visual frames reuse object storage and preserve camera projection', async () => {
    const harness = await browserHarness();
    harness.evaluate(`app.nodes = [{id:'sample',kind:'project',label:'Sample',x:100,y:100}];
        app.edges = []; app.positions = [100,100]; app.state = {semantic_hash:'0123456789abcdef'};
        app.size = {fit:1,width:1800,height:1680}; app.camera = {x:15,y:-5,zoom:2};
        globalThis.palette = {personal:'#ffffff'};
        globalThis.first = buildVisualFrame(0,palette); globalThis.item = first.nodes[0];`);

    const result = harness.evaluate(`(() => {const next = buildVisualFrame(1,palette);
        const expected = logicalToScreen(100,100);
        return [next === first,next.nodes[0] === item,item.x === expected.x,item.y === expected.y];})()`);

    assert.deepEqual([...result], [true,true,true,true]);
});

test('organization identity links maintainer and derives namespaces from state', async () => {
    const harness = await browserHarness();
    const identity = harness.context.document.querySelector('.identity');
    const label = {textContent:''};
    identity.querySelector = () => label;
    harness.evaluate(`globalThis.organization = {
        profile:{variant:'organization',organization:'example-org',username:'unused',
            source_url:'https://github.com/example-org/.github',tagline:'Example',
            maintainer:{username:'maintainer',role:'Core maintainer',url:'https://github.com/maintainer'}},
        nodes:[{id:'domain:example-org',kind:'domain',domain:'example-org',scope:'organization',label:'Example'},
            {id:'domain:second',kind:'domain',domain:'second',scope:'organization',label:'Second'}],edges:[]
    }`);

    harness.evaluate('configureIdentity(organization)');
    const links = harness.elements.get('.footer-links').children;

    assert.equal(identity.href,'https://github.com/example-org');
    assert.equal(label.textContent,'/ example-org');
    assert.ok(links.some(link => link.href === 'https://github.com/maintainer'));
    assert.deepEqual([...harness.evaluate('app.domainCycle')], ['all','example-org','second']);
    assert.equal(harness.evaluate("app.organizationDomains.has('second')"),true);
});

for (const variant of ['personal', 'organization']) {
    test(`profile chrome derives ${variant} styling from the variant with arbitrary owners`, async () => {
        // Arrange
        const harness = await browserHarness();
        const identity = harness.context.document.querySelector('.identity');
        identity.querySelector = () => ({ textContent: '' });
        harness.context.identityState = {
            profile: { variant, username: 'independent-person', organization: 'independent-labs',
                source_url: 'https://github.com/independent-person/profile', tagline: 'Profile' },
            nodes: [], edges: [],
        };

        // Act
        harness.evaluate('configureIdentity(identityState)');

        // Assert
        assert.equal(harness.context.document.documentElement.dataset.profileVariant, variant);
        assert.equal(harness.context.document.title,
            `SOURCEFIELD / ${variant === 'organization' ? 'independent-labs' : 'independent-person'}`);
    });
}

test('fallback rejects canvas sizes that overflow force calculations', () => {
    const anchors = [10,20];

    const construct = () => new Simulator(anchors,anchors,[],[],1,Number.MAX_VALUE,100);

    assert.throws(construct,RangeError);
});

test('out-of-order history completion cannot replace the newest state or engine', async () => {
    const harness = await browserHarness();
    const hashes = ['1111111111111111','2222222222222222'];
    const base = {schema_version:3,profile:{variant:'personal'},stats:{},mode:'preview',
        canvas:{width:100,height:100},nodes:[{id:'sample',label:'Sample',x:10,y:20}],edges:[]};
    const waiting = new Map();
    const freed = [];
    harness.context.fetch = async path => ({ok:true,json:async () => path.endsWith('index.json')
        ? {schema_version:3,states:hashes.map(hash => ({hash,file:`${hash}.json`}))}
        : {...base,semantic_hash:path.includes(hashes[0]) ? hashes[0] : hashes[1]}});
    harness.context.pending = state => new Promise(resolve => waiting.set(state.semantic_hash,resolve));
    harness.evaluate(`createSimulator = pending; configureIdentity = () => {}; renderMetrics = () => {};
        renderNavigation = () => {}; resetView = () => {}; updateLayer = () => {};
        app.simulator = {free() {}}; app.renderer = {name:'test'};`);
    await harness.evaluate('loadHistory()');
    const select = harness.elements.get('#history-select');
    const change = harness.handlers.get('#history-select:change');

    select.value = hashes[0];
    const first = change();
    await new Promise(resolve => setImmediate(resolve));
    select.value = hashes[1];
    const second = change();
    await new Promise(resolve => setImmediate(resolve));
    waiting.get(hashes[1])({engineLabel:'newest',free() {freed.push('newest');}});
    await second;
    waiting.get(hashes[0])({engineLabel:'stale',free() {freed.push('stale');}});
    await first;

    assert.equal(harness.evaluate('app.state.semantic_hash'),hashes[1]);
    assert.equal(harness.evaluate('app.engine'),'newest');
    assert.deepEqual(freed,['stale']);
});


test('paused exploration redraws only when interaction invalidates the frame', async () => {
    const harness = await browserHarness();
    let renders = 0;
    harness.context.requestAnimationFrame = () => {};
    harness.context.renderer = {render() {renders++;}};
    harness.evaluate(`app.layer='systems'; app.running=false; app.renderer=renderer;
        app.positions=[]; readPalette=() => ({}); buildVisualFrame=() => ({}); drawOverlay=() => {};`);

    harness.evaluate('frame(16); frame(32); frame(48)');

    assert.equal(renders,1);
    assert.equal(harness.evaluate('app.dirty'),false);
});


for (const reducedMotion of [false, true]) {
    test(`hover exit paints once without simulation: ${reducedMotion ? 'reduced-motion' : 'paused'}`, async () => {
        // Arrange
        const harness = await browserHarness({ reducedMotion });
        let renders = 0;
        let advances = 0;
        harness.context.requestAnimationFrame = () => {};
        harness.context.renderer = { render() { renders++; } };
        harness.context.simulator = { advance() { advances++; } };
        harness.evaluate(`app.layer = 'systems'; app.running = false; app.renderer = renderer;
            app.simulator = simulator; app.positions = []; app.hovered = { id: 'project:sample' };
            app.dirty = false; buildVisualFrame = () => ({}); drawOverlay = () => {}; app.palette = {};`);

        // Act
        harness.handlers.get('#stage:pointerleave')();
        harness.evaluate('frame(16); frame(32); frame(48)');

        // Assert
        assert.equal(harness.evaluate('app.hovered'), null);
        assert.equal(renders, 1);
        assert.equal(advances, 0);
        assert.equal(harness.evaluate('app.running'), false);
        assert.equal(harness.evaluate('app.dirty'), false);
    });
}

test('leaving an unhovered paused field does not repaint', async () => {
    // Arrange
    const harness = await browserHarness();
    let renders = 0;
    harness.context.requestAnimationFrame = () => {};
    harness.context.renderer = { render() { renders++; } };
    harness.evaluate(`app.layer = 'systems'; app.running = false; app.renderer = renderer;
        app.positions = []; app.dirty = false; app.palette = {};
        buildVisualFrame = () => ({}); drawOverlay = () => {};`);

    // Act
    harness.handlers.get('#stage:pointerleave')();
    harness.evaluate('frame(16); frame(32)');

    // Assert
    assert.equal(renders, 0);
    assert.equal(harness.evaluate('app.dirty'), false);
});

test('leaving during a captured gesture preserves hover and paused idle state', async () => {
    // Arrange
    const harness = await browserHarness();
    harness.evaluate(`app.running = false; app.pointer.down = true; app.dirty = false;
        app.hovered = { id: 'project:sample' };`);

    // Act
    harness.handlers.get('#stage:pointerleave')();

    // Assert
    assert.equal(harness.evaluate('app.hovered.id'), 'project:sample');
    assert.equal(harness.evaluate('app.dirty'), false);
});

test('organization inspector exposes approved maintainer attribution', async () => {
    const harness = await browserHarness();
    harness.evaluate(`app.state={organizations:[{id:'example',maintainer:{username:'maintainer',role:'Core maintainer',url:'https://github.com/maintainer'}}]};
        globalThis.node={id:'project:example:sample',kind:'project',domain:'example',label:'Sample',summary:'Approved description'};`);

    harness.evaluate('openDetails(node)');
    const items = harness.elements.get('#detail-list').children;

    assert.equal(items.length,1);
    assert.equal(items[0].children[0].href,'https://github.com/maintainer');
    assert.equal(items[0].children[0].textContent,'maintainer / Core maintainer');
});

test('organization state permits approved high-level private organization projects', async () => {
    const harness = await browserHarness();
    harness.evaluate(`globalThis.organization = {
        schema_version:3,profile:{variant:'organization'},stats:{},mode:'preview',semantic_hash:'0123456789abcdef',
        canvas:{width:100,height:100},edges:[],nodes:[
            {id:'domain:example',kind:'domain',scope:'organization',domain:'example',label:'Example',x:10,y:20},
            {id:'project:example:private',kind:'project',scope:'private',domain:'example',visibility:'private',label:'Approved project',x:30,y:40}]
    }`);

    const count = harness.evaluate('validateState(organization).nodes.length');

    assert.equal(count,2);
});

test('uppercase producer history hashes select canonical lowercase filenames', async () => {
    const harness = await browserHarness();
    const hash = 'ABCDEF0123456789';
    harness.context.fetch = async () => ({ok:true,json:async () => ({schema_version:3,
        states:[{hash,file:`${hash.toLowerCase()}.json`,generated_at:'2026-10-03T00:00:00Z'}]})});

    await harness.evaluate('loadHistory()');
    const select = harness.elements.get('#history-select');

    assert.notEqual(select.disabled,true);
    assert.equal(select.children[0]?.value,hash.toLowerCase());
});

test('closing an already closed inspector does not restore stale focus', async () => {
    const harness = await browserHarness();
    let focuses = 0;
    harness.context.initiator = {isConnected:true,focus() {focuses++;}};
    harness.evaluate('app.returnFocus=initiator');

    harness.evaluate('closeDetails()');

    assert.equal(focuses,0);
    assert.equal(harness.evaluate('app.returnFocus'),null);
});

test('organization metrics replace unavailable followers with package downloads', async () => {
    const harness = await browserHarness();
    harness.evaluate("app.state={profile:{variant:'organization'},mode:'live',stats:{organization_public_repositories:2,package_count:4,package_downloads:1200,followers:null}}");

    harness.evaluate('renderMetrics()');
    const signals = harness.elements.get('#metrics').children;

    assert.ok(signals.some(item => item.children[0] === 'DOWNLOADS ' && item.children[1].textContent === '1200'));
    assert.ok(signals.every(item => item.children[0] !== 'FOLLOWERS '));
});

for (const [name, entry] of [
    ['traversal',{hash:'ABCDEF0123456789',file:'../abcdef0123456789.json'}],
    ['mismatched identity',{hash:'ABCDEF0123456789',file:'0000000000000000.json'}],
    ['malformed hash',{hash:'not-a-hash',file:'not-a-hash.json'}],
]) {
    test(`history rejects ${name} before adding any options`, async () => {
        const harness = await browserHarness();
        harness.context.index={schema_version:3,states:[entry]};

        const parse = () => harness.evaluate('validatedHistoryEntries(index)');

        assert.throws(parse,/Invalid history/);
    });
}

test('history rejects duplicate hashes differing only in case', async () => {
    const harness=await browserHarness();
    harness.context.index={schema_version:3,states:[
        {hash:'ABCDEF0123456789',file:'abcdef0123456789.json'},
        {hash:'abcdef0123456789',file:'abcdef0123456789.json'},
    ]};

    const parse=() => harness.evaluate('validatedHistoryEntries(index)');

    assert.throws(parse,/duplicate identity/);
});

test('closing details restores a connected initiator and clears its reference', async () => {
    const harness=await browserHarness();
    let focused=0;
    harness.context.initiator={isConnected:true,focus() {focused++;}};
    harness.evaluate('app.detailsOpen=true; app.returnFocus=initiator');

    harness.evaluate('closeDetails()');

    assert.equal(focused,1);
    assert.equal(harness.evaluate('app.returnFocus'),null);
    assert.equal(harness.evaluate('app.detailsOpen'),false);
});

test('detached inspector initiator falls back to an available layer control', async () => {
    const harness=await browserHarness();
    let focused=0;
    let detachedFocus=0;
    harness.context.document.querySelector('[data-layer="systems"]').focus=() => {focused++;};
    harness.context.initiator={isConnected:false,focus() {detachedFocus++;}};
    harness.evaluate('app.detailsOpen=true; app.returnFocus=initiator');

    harness.evaluate('closeDetails()');

    assert.equal(focused,1);
    assert.equal(detachedFocus,0);
});

test('switching inspector content preserves its original external initiator', async () => {
    const harness=await browserHarness();
    const initiator={isConnected:true};
    harness.context.initiator=initiator;
    harness.elements.get('#detail-panel').contains=() => true;
    harness.evaluate('app.detailsOpen=true; app.returnFocus=initiator; app.state={};');

    harness.evaluate("openDetails({id:'next',kind:'project',label:'Next',summary:'Example'})");

    assert.strictEqual(harness.evaluate('app.returnFocus'),initiator);
});

test('personal ownership uses domain scope with arbitrary identifiers', async () => {
    const harness=await browserHarness();
    harness.context.document.querySelector('.identity').querySelector=() => ({textContent:''});
    harness.evaluate(`globalThis.state={profile:{variant:'personal',username:'person',source_url:'https://github.com/person/person'},
        nodes:[{id:'domain:my-space',kind:'domain',domain:'my-space',scope:'personal',label:'Person',url:'https://github.com/person'},
            {id:'domain:first',kind:'domain',domain:'first',scope:'organization',label:'First',url:'https://github.com/first'},
            {id:'domain:second',kind:'domain',domain:'second',scope:'organization',label:'Second',url:'https://github.com/second'}],edges:[]};`);

    harness.evaluate('configureIdentity(state)');
    const links=harness.elements.get('.footer-links').children;

    assert.equal(links.filter(link => link.href==='https://github.com/person').length,1);
    assert.deepEqual([...harness.evaluate('app.domainCycle')],['all','my-space','first','second']);
    assert.deepEqual([...harness.evaluate('app.organizationDomains')],['first','second']);
    assert.equal(harness.evaluate('app.domainNodes.length'),3);
});

test('organization metrics preserve known zero downloads and available followers', async () => {
    const harness=await browserHarness();
    harness.evaluate("app.state={profile:{variant:'organization'},mode:'live',stats:{package_downloads:0,followers:0}}");

    harness.evaluate('renderMetrics()');
    const signals=harness.elements.get('#metrics').children;

    assert.equal(signals.find(item => item.children[0]==='DOWNLOADS ').children[1].textContent,'0');
    assert.equal(signals.find(item => item.children[0]==='FOLLOWERS ').children[1].textContent,'0');
});

test('theme transitions refresh cached font and color tokens exactly once', async () => {
    const harness=await browserHarness();
    let reads=0;
    let theme='dark';
    harness.context.getComputedStyle=() => {reads++; return {getPropertyValue:key => `${theme}-${key}`};};
    harness.evaluate('app.palette=readPalette(); loadProfile=() => {};');
    const initial=harness.evaluate('app.palette.mono');

    theme='light';
    harness.handlers.get('(prefers-color-scheme: light):change')();

    assert.equal(reads,2);
    assert.equal(initial,'dark---mono');
    assert.equal(harness.evaluate('app.palette.mono'),'light---mono');
    assert.equal(harness.evaluate('app.palette.text'),'light---text');
});

test('history rejects a different archive identity before replacing current state', async () => {
    const harness = await browserHarness();
    const hash = 'ABCDEF0123456789';
    const archive = {
        schema_version: 3, profile: { variant: 'personal' }, stats: {}, mode: 'preview',
        semantic_hash: 'FFFFFFFFFFFFFFFF', canvas: { width: 100, height: 100 },
        nodes: [{ id: 'sample', label: 'Sample', x: 10, y: 20 }], edges: [],
    };

    harness.context.fetch = async path => ({ ok: true, json: async () => path.endsWith('index.json')
        ? { schema_version: 3, states: [{ hash, file: `${hash.toLowerCase()}.json` }] } : archive });
    harness.evaluate("app.state = { semantic_hash: 'CURRENT' };");
    await harness.evaluate('loadHistory()');
    harness.elements.get('#history-select').value = hash.toLowerCase();

    await harness.handlers.get('#history-select:change')();

    assert.equal(harness.evaluate('app.state.semantic_hash'), 'CURRENT');
    assert.equal(harness.elements.get('#state-label').textContent, 'SNAPSHOT UNAVAILABLE');
});

test('history rejects a forged selection without requesting its path', async () => {
    const harness = await browserHarness();
    const paths = [];
    harness.context.fetch = async path => {
        paths.push(path);

        return { ok: true, json: async () => ({ schema_version: 3, states: [] }) };
    };
    await harness.evaluate('loadHistory()');
    harness.elements.get('#history-select').value = '../other';

    await harness.handlers.get('#history-select:change')();

    assert.deepEqual(paths, ['./history/index.json']);
    assert.equal(harness.elements.get('#state-label').textContent, 'SNAPSHOT UNAVAILABLE');
});

/** Observe the XML parser boundary without treating the stub as SVG validation. */
async function profileHarness(source) {
    const harness = await browserHarness();
    const parses = [];
    const svg = { localName: 'svg', attributes: [], querySelectorAll: () => [] };
    harness.context.source = source;
    harness.context.DOMParser = class {
        parseFromString(markup, type) {
            parses.push({ markup, type });
            return { documentElement: svg, querySelector: () => null };
        }
    };
    harness.context.document.importNode = node => node;

    return { ...harness, parses, svg };
}

for (const [name, source, expected] of [
    ['ordinary geometry', '<svg><rect width="4" /></svg>', '<svg><rect width="4" /></svg>'],
    ['generated CSS', '<svg><style>rect { fill: red; }</style><rect /></svg>', '<svg><rect /></svg>'],
    ['multiple CSS blocks', '<svg><style type="text/css">a {}</style><STYLE>rect {}</STYLE></svg>', '<svg></svg>'],
    ['CSS CDATA', '<svg><style><![CDATA[rect { fill: red; }]]></style><rect /></svg>', '<svg><rect /></svg>'],
    ['text mentioning style', '<svg><text> style=example </text></svg>', '<svg><text> style=example </text></svg>'],
    ['quoted metadata', '<svg data-note=" > style=example" />', '<svg data-note=" > style=example" />'],
]) {
    test(`profile preprocessing preserves ${name} with one XML parse`, async () => {
        const harness = await profileHarness(source);

        const svg = harness.evaluate('parseProfileSvg(source)');

        assert.equal(svg, harness.svg);
        assert.deepEqual(harness.parses, [{ markup: expected, type: 'image/svg+xml' }]);
    });
}

for (const [name, body] of [
    ['recreated style tag', '<sty<style>discard</style>le>rect { fill: red; }</sty<style>discard</style>le>'],
    ['nested style tag', '<style><style>discard</style>rect { fill: red; }</style>'],
    ['unclosed style tag', '<style>rect { fill: red; }'],
    ['unpaired closing style tag', '</style>'],
    ['Unicode-qualified style tag', '<\u03c0:style>rect {}</\u03c0:style>'],
    ['self-closing style tag', '<style />'],
    ['qualified style tag', '<s:style xmlns:s="http://www.w3.org/2000/svg">rect {}</s:style>'],
    ['qualified uppercase style tag', '<s:STYLE xmlns:s="http://www.w3.org/2000/svg">rect {}</s:STYLE>'],
    ['stylesheet instruction', '<?xml-stylesheet type="text/css" href="https://example.invalid/probe.css"?>'],
    ['inline style attribute', '<rect style="fill: red" />'],
    ['qualified style attribute', '<rect s:style="fill: red" xmlns:s="http://www.w3.org/2000/svg" />'],
    ['style attribute after a quoted angle', '<rect data-note=">" style="fill: red" />'],
    ['single-quoted style attribute', "<rect style='fill: red' />"],
    ['multiline style attribute', '<rect\nstyle\t=\n"fill: red" />'],
]) {
    test(`profile preprocessing rejects ${name} before XML parsing`, async () => {
        const harness = await profileHarness(`<svg>${body}</svg>`);
        let failure;

        try {
            harness.evaluate('parseProfileSvg(source)');
        } catch (error) {
            failure = error.message;
        }

        assert.equal(failure, 'Invalid profile SVG');
        assert.deepEqual(harness.parses, []);
    });
}

test('profile preprocessing rejects entity declarations before XML parsing', async () => {
    const harness = await profileHarness(
        '<!DOCTYPE svg [<!ENTITY css "&#60;style&#62;rect {}&#60;/style&#62;">]><svg>&css;</svg>',
    );
    let failure;

    try {
        harness.evaluate('parseProfileSvg(source)');
    } catch (error) {
        failure = error.message;
    }

    assert.equal(failure, 'Invalid profile SVG');
    assert.deepEqual(harness.parses, []);
});

for (const [name, body] of [
    ['unclosed tags', '<style>'.repeat(50000)], ['unterminated tags', '<style '.repeat(50000)],
]) {
    test(`profile preprocessing rejects 50000 ${name} before parsing`, async () => {
        const harness = await profileHarness(`<svg>${body}</svg>`);
        let failure;

        try {
            harness.evaluate('parseProfileSvg(source)');
        } catch (error) {
            failure = error.message;
        }

        assert.equal(failure, 'Invalid profile SVG');
        assert.deepEqual(harness.parses, []);
    });
}

test('profile preprocessing preserves geometry across 10000 independent CSS blocks', async () => {
    const harness = await profileHarness(`<svg>${'<style>rect {}</style><rect />'.repeat(10000)}</svg>`);

    harness.evaluate('parseProfileSvg(source)');

    assert.equal(harness.parses.length, 1);
    assert.equal(harness.parses[0].markup, `<svg>${'<rect />'.repeat(10000)}</svg>`);
});

for (const body of ['<stylesheet />', '<styleable />', '<s:stylesheet xmlns:s="http://www.w3.org/2000/svg" />']) {
    test(`profile preprocessing preserves non-style XML names: ${body}`, async () => {
        const source = `<svg>${body}</svg>`;
        const harness = await profileHarness(source);

        harness.evaluate('parseProfileSvg(source)');

        assert.deepEqual(harness.parses, [{ markup: source, type: 'image/svg+xml' }]);
    });
}
