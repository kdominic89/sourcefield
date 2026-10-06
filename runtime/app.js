import { Simulator as FallbackSimulator } from './simulation-fallback.js';

const TAU = Math.PI * 2;
let LOGICAL_WIDTH = 1800;
let LOGICAL_HEIGHT = 1680;
const mediaReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const mediaLight = window.matchMedia('(prefers-color-scheme: light)');

const stage = document.querySelector('#stage');
let glCanvas = document.querySelector('#gl-canvas');
const labelCanvas = document.querySelector('#label-canvas');
const intro = document.querySelector('#intro');
const engineLabel = document.querySelector('#engine-label');
const stateLabel = document.querySelector('#state-label');
const metrics = document.querySelector('#metrics');
const gestureHint = document.querySelector('#gesture-hint');
const detailPanel = document.querySelector('#detail-panel');
const detailKind = document.querySelector('#detail-kind');
const detailTitle = document.querySelector('#detail-title');
const detailSummary = document.querySelector('#detail-summary');
const detailTags = document.querySelector('#detail-tags');
const detailList = document.querySelector('#detail-list');
const detailLink = document.querySelector('#detail-link');
const domainButton = document.querySelector('#domain-button');
const domainLabel = document.querySelector('#domain-label');
const motionButton = document.querySelector('#motion-button');

const app = {
    state: null,
    dirty: true,
    nodes: [],
    edges: [],
    nodeById: new Map(),
    positions: null,
    simulator: null,
    engine: 'JAVASCRIPT FALLBACK',
    layer: 'overview',
    domain: 'all',
    domainCycle: ['all'],
    domainLabels: new Map(),
    organizationDomains: new Set(),
    domainNodes: [],
    detailsOpen: false,
    returnFocus: null,
    palette: null,
    selected: null,
    hovered: null,
    running: !mediaReducedMotion.matches,
    reducedMotion: mediaReducedMotion.matches,
    userPaused: false,
    lastTime: performance.now(),
    pointer: { x: 0, y: 0, down: false, moved: false, startX: 0, startY: 0, panX: 0, panY: 0 },
    camera: { x: 0, y: 0, zoom: 1 },
    size: { width: 1, height: 1, dpr: 1, fit: 1 },
    renderer: null,
};



/** Load canonical state, initialize the selected engine, and start the visual loop. */
async function boot() {
    const response = await fetch('./profile-state.json', { cache: 'no-store' });
    if (!response.ok) throw new Error(`State ${response.status}`);
    app.state = validateState(await response.json());
    LOGICAL_WIDTH = app.state.canvas.width;
    LOGICAL_HEIGHT = app.state.canvas.height;
    app.nodes = app.state.nodes;
    configureIdentity(app.state);
    document.querySelector('#mobile-identity').textContent = app.state.profile?.tagline ?? '';
    app.edges = app.state.edges;
    app.nodeById = new Map(app.nodes.map((node, index) => [node.id, { node, index }]));
    app.positions = new Float32Array(app.nodes.flatMap(node => [node.x, node.y]));

    app.simulator = await createSimulator();
    app.engine = app.simulator.engineLabel;
    app.renderer = createRenderer(glCanvas);
    app.palette = readPalette();
    detailPanel.inert = true;
    engineLabel.textContent = `${app.engine} / ${app.renderer.name}`;
    stateLabel.textContent = `STATE ${formatHash(app.state.semantic_hash)} / ${app.state.mode.toUpperCase()}`;
    renderMetrics();
    renderNavigation();
    await loadProfile();
    await loadHistory();
    bindEvents();
    updateMotionButton();
    resize();
    requestAnimationFrame(frame);
}

/** Apply public identity using DOM text APIs so authored labels cannot inject markup. */
function configureIdentity(state) {
    app.dirty = true;
    const profile = state.profile;
    const organization = profile.variant === 'organization';
    const handle = organization ? profile.organization : profile.username;
    const identity = document.querySelector('.identity');
    const url = `https://github.com/${encodeURIComponent(handle)}`;
    identity.href = url;
    identity.querySelector('small').textContent = `/ ${handle}`;
    document.title = `SOURCEFIELD / ${handle}`;
    document.querySelector('#mobile-identity').textContent = profile.tagline ?? '';
    app.neighbors = new Map(state.nodes.map(node => [node.id, new Set()]));

    for (const edge of state.edges) {
        app.neighbors.get(edge.from).add(edge.to);
        app.neighbors.get(edge.to).add(edge.from);
    }

    app.domainLabels = new Map();
    app.organizationDomains = new Set();
    app.domainNodes = state.nodes.filter(node => node.kind === 'domain');

    for (const node of app.domainNodes) {
        if (!node.domain) continue;

        app.domainLabels.set(node.domain, node.label);

        if (node.scope === 'organization') app.organizationDomains.add(node.domain);
    }

    app.domainCycle = ['all', ...app.domainLabels.keys()];
    app.domain = 'all';
    domainLabel.textContent = 'All namespaces';
    domainButton.hidden = app.domainLabels.size < 2;
    const links = document.querySelector('.footer-links');
    links.replaceChildren();
    const items = [[handle, url], ['SOURCE', profile.source_url]];

    if (organization && profile.maintainer) {
        items.push([`${profile.maintainer.username} / ${profile.maintainer.role}`, profile.maintainer.url]);
    }

    for (const node of state.nodes) {
        if (node.kind === 'domain' && node.scope === 'organization' &&
            (!organization || node.url !== url)) {
            items.push([node.label, node.url]);
        }
    }

    for (const [label, href] of items) {
        if (!safePublicUrl(href)) continue;

        const link = document.createElement('a');
        link.textContent = label;
        link.href = href;
        link.rel = 'noreferrer';
        links.append(link);
    }
}

/** Prefer the generated Rust module; explicitly label the independent JavaScript fallback. */
async function createSimulator(state = app.state, nodes = app.nodes, edges = app.edges) {
    const positions = new Float32Array(nodes.flatMap(node => [node.x, node.y]));
    const anchors = new Float32Array(positions);
    const nodeById = new Map(nodes.map((node, index) => [node.id, { node, index }]));
    const pairs = [];
    const weights = [];
    for (const edge of edges) {
        const from = nodeById.get(edge.from);
        const to = nodeById.get(edge.to);
        if (!from || !to) continue;
        pairs.push(from.index, to.index);
        weights.push(edge.weight ?? .2);
    }

    const seed = Number.parseInt(state.semantic_hash.slice(0, 8), 16) >>> 0;

    try {
        const wasm = await import('./pkg/sourcefield_wasm.js');
        await wasm.default();
        const simulator = new wasm.Simulator(
            positions,
            anchors,
            new Uint32Array(pairs),
            new Float32Array(weights),
            seed,
            state.canvas.width,
            state.canvas.height,
        );
        simulator.engineLabel = 'RUST/WASM FIELD';

        return simulator;
    } catch (error) {
        // Keep the fallback visibly distinct: a missing binary must never look like WASM evidence.
        const simulator = new FallbackSimulator(positions, anchors, pairs, weights, seed, state.canvas.width, state.canvas.height);
        simulator.engineLabel = 'JAVASCRIPT FALLBACK';

        return simulator;
    }
}

/** Bind controls and isolate navigation from canvas gestures. */
function bindEvents() {
    window.addEventListener('resize', resize, { passive: true });
    document.addEventListener('visibilitychange', () => {
        if (!document.hidden) syncProfileAnimations();
    });
    mediaLight.addEventListener?.('change', () => {
        app.dirty = true;
        app.palette = readPalette();
        app.renderer?.setTheme(app.palette);
        loadProfile();
    });

    mediaReducedMotion.addEventListener?.('change', event => {
        app.reducedMotion = event.matches;
        app.running = !event.matches && !app.userPaused;
        updateMotionButton();
    });

    for (const button of document.querySelectorAll('[data-layer]')) {
        button.addEventListener('click', () => {
            app.layer = button.dataset.layer;
            document.querySelectorAll('[data-layer]').forEach(candidate =>
                candidate.classList.toggle('is-active', candidate === button));
            intro.classList.add('is-muted');
            updateLayer();
        });
    }

    domainButton.addEventListener('click', () => {
        const current = app.domainCycle.indexOf(app.domain);
        app.domain = app.domainCycle[(current + 1) % app.domainCycle.length];
        domainLabel.textContent = app.domain === 'all' ? 'All namespaces' :
            app.domainLabels.get(app.domain) ?? app.domain;
        renderNavigation();
        updateProfileFilter();
        intro.classList.add('is-muted');
    });

    motionButton.addEventListener('click', () => {
        app.running = !app.running;
        app.userPaused = !app.running;
        updateMotionButton();
    });

    document.querySelector('#reset-button').addEventListener('click', resetView);
    document.querySelector('#detail-close').addEventListener('click', closeDetails);

    stage.addEventListener('pointerdown', event => {
        if (ignoreGesture(event) || app.pointer.down || event.isPrimary === false) return;

        updatePointer(event);
        app.pointer.id = event.pointerId;
        app.pointer.down = true;
        app.pointer.moved = false;
        app.pointer.startX = event.clientX;
        app.pointer.startY = event.clientY;
        app.pointer.panX = app.camera.x;
        app.pointer.panY = app.camera.y;
        stage.setPointerCapture(event.pointerId);
        stage.classList.add('is-dragging');
        gestureHint.classList.add('is-hidden');
    });

    stage.addEventListener('pointermove', event => {
        if (ignoreGesture(event) || (app.pointer.down && event.pointerId !== app.pointer.id)) return;

        updatePointer(event);
        if (app.pointer.down) {
            const dx = event.clientX - app.pointer.startX;
            const dy = event.clientY - app.pointer.startY;
            if (Math.hypot(dx, dy) > 3) app.pointer.moved = true;
            app.camera.x = app.pointer.panX + dx;
            app.camera.y = app.pointer.panY + dy;
            intro.classList.add('is-muted');
        } else {
            updateHover();
        }
    });

    stage.addEventListener('pointerup', event => {
        if (!app.pointer.down || event.pointerId !== app.pointer.id) return;

        updatePointer(event);
        stage.classList.remove('is-dragging');
        app.pointer.down = false;
        if (!app.pointer.moved) {
            const hit = hitTest(app.pointer.x, app.pointer.y);
            hit ? openDetails(hit) : closeDetails();
        }

        stage.releasePointerCapture?.(event.pointerId);
    });

    stage.addEventListener('pointercancel', cancelPointer);
    stage.addEventListener('lostpointercapture', cancelPointer);

    stage.addEventListener('pointerleave', () => {
        if (!app.pointer.down) app.hovered = null;
    });

    stage.addEventListener('wheel', event => {
        if (ignoreGesture(event)) return;

        event.preventDefault();
        updatePointer(event);
        const before = screenToLogical(app.pointer.x, app.pointer.y);
        const factor = Math.exp(-event.deltaY * .0012);
        app.camera.zoom = clamp(app.camera.zoom * factor, .55, 2.8);
        const after = logicalToScreen(before.x, before.y);
        app.camera.x += app.pointer.x - after.x;
        app.camera.y += app.pointer.y - after.y;
        intro.classList.add('is-muted');
        gestureHint.classList.add('is-hidden');
    }, { passive: false });

    window.addEventListener('keydown', event => {
        if (event.key === 'Escape') closeDetails();
        if (event.key.toLowerCase() === 'r' && event.target === document.body && !event.metaKey && !event.ctrlKey) {
            resetView();
        }

        if (event.code === 'Space' && event.target === document.body && !app.reducedMotion) {
            event.preventDefault();
            app.running = !app.running;
            app.userPaused = !app.running;
            updateMotionButton();
        }
    });
}

/** Restore canonical anchors, camera scale, and selection. */
function resetView() {
    app.dirty = true;
    app.camera.x = 0;
    app.camera.y = 0;
    app.camera.zoom = 1;
    app.positions = Float32Array.from(app.simulator.reset());
    app.selected = null;
    app.hovered = null;
    closeDetails();
    intro.classList.remove('is-muted');
}

/** Keep actual motion, accessibility state, and the initial system preference consistent. */
function updateMotionButton() {
    app.dirty = true;
    if (app.reducedMotion) app.running = false;

    document.body.classList.toggle('motion-paused', !app.running);
    document.querySelector('#profile-view svg')?.classList.toggle('paused', !app.running);
    motionButton.disabled = app.reducedMotion;
    motionButton.setAttribute('aria-pressed', String(!app.running));
    motionButton.title = app.reducedMotion ? 'Motion disabled by system preference' :
        app.running ? 'Pause motion' : 'Resume motion';
    motionButton.setAttribute('aria-label', motionButton.title);
    motionButton.innerHTML = app.running
        ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6v12M16 6v12"/></svg>'
        : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 9 6-9 6Z"/></svg>';

    syncProfileAnimations();
}

/** Render unknown counters as unavailable rather than a fabricated zero. */
function renderMetrics() {
    const stats = app.state.stats;
    const status = document.querySelector('#data-status');
    status.textContent = `${app.state.mode} metadata / ${app.state.generated_at ?? 'date unavailable'}`;

    const signals = [
        ...(app.state.profile?.variant === 'organization' ? [] : [['PERSONAL', stats.personal_public_repositories]]),
        ['ORG', stats.organization_public_repositories],
        ['PACKAGES', stats.package_count],
        ...(app.state.profile?.variant === 'organization'
            ? [['DOWNLOADS', stats.package_downloads], ...(stats.followers == null ? [] : [['FOLLOWERS', stats.followers]])]
            : [['FOLLOWERS', stats.followers]]),
    ];

    // Absence means collection was disabled or unavailable; a known zero is still an observation.
    if (app.state.profile?.variant !== 'organization' && stats.private_repository_count != null) {
        signals.push(['TOKEN-VISIBLE OWNED PRIVATE', stats.private_repository_count]);
    }

    metrics.replaceChildren(...signals.map(([label, value]) => {
        const item = document.createElement('span');
        const count = document.createElement('b');
        count.textContent = Number.isFinite(value) ? String(value) : 'unavailable';
        item.append(`${label} `, count);

        return item;
    }));
}

/** Project declared canvas dimensions into the actual stage and device pixel ratio. */
function resize() {
    app.dirty = true;
    if (window.matchMedia('(max-width: 700px)').matches) {
        document.querySelector('.semantic-profile details').open = true;
    }

    const rect = stage.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    app.size = {
        width: Math.max(1, rect.width),
        height: Math.max(1, rect.height),
        dpr,
        fit: Math.min(rect.width / LOGICAL_WIDTH, rect.height / LOGICAL_HEIGHT) * 1.08,
    };

    for (const canvas of [glCanvas, labelCanvas]) {
        canvas.width = Math.round(rect.width * dpr);
        canvas.height = Math.round(rect.height * dpr);
    }

    app.renderer.resize(app.size);
    syncProfileAnimations();
}

/** Advance permitted motion and render the current visual state. */
function frame(now) {
    const delta = Math.min(.05, Math.max(0, (now - app.lastTime) / 1000));
    app.lastTime = now;
    // The approved overview uses fixed radial endpoints; only its rings and signals animate.
    // Simulation belongs to the exploratory layers, where nodes and edges share live coordinates.
    if (app.layer !== 'overview' && (app.dirty || (app.running && !app.reducedMotion))) {
        if (app.running && !app.reducedMotion) {
            app.simulator.advance(delta);

            // Scalar reads avoid unsafe views invalidated by WASM memory growth and frame copies.
            for (let index = 0; index < app.positions.length; index++) {
                app.positions[index] = app.simulator.coordinate(index);
            }
        }

        const palette = app.palette ?? readPalette();
        const visual = buildVisualFrame(now / 1000, palette);
        app.renderer.render(visual, palette, app.size);
        drawOverlay(visual, palette, now / 1000);
        app.dirty = false;
    }

    requestAnimationFrame(frame);
}

/** Resolve graph positions, visibility, and particles for either graphics backend. */
function buildVisualFrame(time, palette) {
    const frame = app.visualFrame ??= { nodes: [], edges: [], particles: [] };
    const nodePool = app.visualNodes ??= [];
    const edgePool = app.visualEdges ??= [];
    const particlePool = app.visualParticles ??= [];
    const { nodes, edges, particles } = frame;
    nodes.length = 0;
    edges.length = 0;
    particles.length = 0;
    if (app.visibility?.length !== app.nodes.length) {
        app.visibility = new Float32Array(app.nodes.length);
    }

    const visibility = app.visibility;

    for (let index = 0; index < app.nodes.length; index++) {
        const node = app.nodes[index];
        const alpha = nodeAlpha(node);
        visibility[index] = alpha;
        if (alpha <= .005) continue;

        const item = nodePool[index] ??= {};
        projectPosition(app.positions[index * 2], app.positions[index * 2 + 1], item);
        const selected = app.selected?.id === node.id;
        const hovered = app.hovered?.id === node.id;
        const connected = isConnectedToFocus(node.id);
        const dim = (app.selected || app.hovered) && !selected && !hovered && !connected ? .18 : 1;
        item.node = node;
        item.index = index;
        item.size = nodeSize(node) * app.size.fit * app.camera.zoom;
        item.color = nodeColor(node, palette);
        item.alpha = alpha * dim;
        item.selected = selected;
        item.hovered = hovered;
        item.connected = connected;
        nodes.push(item);
    }

    for (let index = 0; index < app.edges.length; index++) {
        const edge = app.edges[index];
        const fromRef = app.nodeById.get(edge.from);
        const toRef = app.nodeById.get(edge.to);
        if (!fromRef || !toRef) continue;
        const alpha = Math.min(visibility[fromRef.index], visibility[toRef.index]);
        if (alpha <= .01) continue;
        const item = edgePool[index] ??= { from: {}, to: {} };
        projectPosition(app.positions[fromRef.index * 2], app.positions[fromRef.index * 2 + 1], item.from);
        projectPosition(app.positions[toRef.index * 2], app.positions[toRef.index * 2 + 1], item.to);
        const focused = isFocusedEdge(edge);
        const dim = (app.selected || app.hovered) && !focused ? .08 : 1;
        item.edge = edge;
        item.color = edgeColor(edge, palette);
        item.alpha = alpha * dim * edgeAlpha(edge);
        item.index = index;
        edges.push(item);
    }

    if (app.running && !app.reducedMotion) {
        for (const item of edges) {
            if (item.alpha < .06 || item.index % 2) continue;
            const speed = .035 + ((item.index * 17) % 11) * .002;
            const t = (time * speed + seededFraction(app.state.semantic_hash, item.index)) % 1;
            const particle = particlePool[item.index] ??= {};
            particle.x = lerp(item.from.x, item.to.x, t);
            particle.y = lerp(item.from.y, item.to.y, t);
            particle.size = item.edge.kind === 'publishes' ? 4.2 : 3.2;
            particle.color = item.color;
            particle.alpha = Math.min(1, item.alpha * 2.4);
            particles.push(particle);
        }
    }

    return frame;
}

/** Resolve layer and namespace visibility without changing canonical state. */
function nodeAlpha(node) {
    let alpha = 1;
    if (app.layer === 'overview') {
        if (node.kind === 'technology' && !node.show_in_readme) alpha = 0;
        else if (node.kind === 'component' || node.kind === 'package') alpha = .78;
        else if (node.kind === 'domain') alpha = .18;
    } else if (app.layer === 'systems') {
        if (node.kind === 'technology' && !node.show_in_readme) alpha = .05;
        else if (node.kind === 'domain') alpha = .12;
    } else if (app.layer === 'capabilities') {
        if (node.kind === 'technology') alpha = node.show_in_readme ? 1 : .68;
        else if (node.kind === 'project' || node.kind === 'publication') alpha = .72;
        else if (node.kind === 'component' || node.kind === 'package') alpha = .26;
        else alpha = .08;
    }

    if (app.domain !== 'all') {
        if (node.domain === app.domain) alpha *= 1;
        else if (node.domain == null && node.kind === 'technology') alpha *= .74;
        else alpha *= .06;
    }

    return alpha;
}

/** Emphasize relationship types relevant to the selected layer. */
function edgeAlpha(edge) {
    if (app.layer === 'overview') return edge.show_in_readme ? .58 : .10;
    if (app.layer === 'systems') return ['component', 'publishes', 'contains'].includes(edge.kind) ? .62 : .18;

    return ['implemented-with', 'integrates', 'targets', 'shared-capability', 'affinity']
        .includes(edge.kind) ? .48 : .12;
}

/** Assign visual weights by semantic kind rather than specific project identity. */
function nodeSize(node) {
    switch (node.kind) {
        case 'project': return 25;
        case 'publication': return 34;
        case 'component': return 8;
        case 'package': return 8;
        case 'technology': return node.show_in_readme ? 11 : 5;
        case 'domain': return 8;
        default: return 5;
    }
}

/** Map ownership and kind to the active theme. */
function nodeColor(node, palette) {
    if (node.kind === 'technology' && node.domain == null) return palette.shared;
    if (app.organizationDomains.has(node.domain)) return node.kind === 'package' ? palette.organization2 : palette.organization;
    if (node.kind === 'component' || node.kind === 'technology') return palette.personal2;

    return palette.personal;
}

/** Color relationships consistently across both graphics backends. */
function edgeColor(edge, palette) {
    if (edge.kind === 'shared-capability') return palette.shared;
    if (edge.kind === 'publishes') return palette.organization2;
    const to = app.nodeById.get(edge.to)?.node;
    if (app.organizationDomains.has(to?.domain)) return palette.organization;

    return edge.kind === 'implemented-with' ? palette.personal2 : palette.quiet;
}

/** Write projected coordinates into retained frame objects to avoid animation garbage. */
function projectPosition(x, y, output) {
    const scale = app.size.fit * app.camera.zoom;
    output.x = (x - LOGICAL_WIDTH / 2) * scale + app.size.width / 2 + app.camera.x;
    output.y = (y - LOGICAL_HEIGHT / 2) * scale + app.size.height / 2 + app.camera.y;
}

/** Convert canonical coordinates through camera pan and zoom. */
function logicalToScreen(x, y) {
    const scale = app.size.fit * app.camera.zoom;

    return {
        x: (x - LOGICAL_WIDTH / 2) * scale + app.size.width / 2 + app.camera.x,
        y: (y - LOGICAL_HEIGHT / 2) * scale + app.size.height / 2 + app.camera.y,
    };
}

/** Invert the camera transform for cursor-centered zoom. */
function screenToLogical(x, y) {
    const scale = app.size.fit * app.camera.zoom;

    return {
        x: (x - app.size.width / 2 - app.camera.x) / scale + LOGICAL_WIDTH / 2,
        y: (y - app.size.height / 2 - app.camera.y) / scale + LOGICAL_HEIGHT / 2,
    };
}

/** Choose the nearest visible node inside a usable pointer target. */
function hitTest(x, y) {
    let best = null;
    let bestDistance = Infinity;
    for (let index = 0; index < app.nodes.length; index++) {
        const node = app.nodes[index];
        if (nodeAlpha(node) < .08 || node.kind === 'domain') continue;
        const position = logicalToScreen(app.positions[index * 2], app.positions[index * 2 + 1]);
        const distance = Math.hypot(position.x - x, position.y - y);
        const threshold = Math.max(13, nodeSize(node) * app.size.fit * app.camera.zoom * .75);
        if (distance <= threshold && distance < bestDistance) {
            best = node;
            bestDistance = distance;
        }
    }

    return best;
}

/** Refresh visual focus only when the pointer changes its nearest node. */
function updateHover() {
    app.dirty = true;
    const next = hitTest(app.pointer.x, app.pointer.y);
    if (next?.id !== app.hovered?.id) {
        app.hovered = next;
        stage.classList.toggle('has-hover', Boolean(next));
    }
}

/** Determine whether a node has a direct relationship to the current focus. */
function isConnectedToFocus(nodeId) {
    const focus = app.selected?.id ?? app.hovered?.id;
    if (!focus) return false;

    return app.neighbors?.get(focus)?.has(nodeId) ?? false;
}

/** Determine whether a relationship touches the current focus. */
function isFocusedEdge(edge) {
    const focus = app.selected?.id ?? app.hovered?.id;

    return !focus || edge.from === focus || edge.to === focus;
}

/** Present approved node content using text nodes and safe public links. */
function openDetails(node) {
    app.dirty = true;
    app.selected = node;
    detailKind.textContent = `${node.kind}${node.scope ? ` / ${node.scope}` : ''}`;
    detailTitle.textContent = node.label;
    detailSummary.textContent = node.summary;
    detailTags.replaceChildren(...(node.tags ?? []).slice(0, 12).map(value => {
        const element = document.createElement('span');
        element.textContent = value;

        return element;
    }));
    detailList.replaceChildren(...(node.details ?? []).map(value => {
        const element = document.createElement('li');
        element.textContent = value;

        return element;
    }));
    const maintainer = app.state?.organizations?.find(organization => organization.id === node.domain)?.maintainer;

    if (maintainer && safePublicUrl(maintainer.url)) {
        const item = document.createElement('li');
        const link = document.createElement('a');
        link.href = maintainer.url;
        link.rel = 'noreferrer';
        link.textContent = `${maintainer.username} / ${maintainer.role}`;
        item.append(link);
        detailList.append(item);
    }

    detailLink.hidden = !node.url;
    if (node.url && safePublicUrl(node.url)) detailLink.href = node.url;
    else detailLink.hidden = true;
    detailPanel.classList.add('is-open');
    detailPanel.setAttribute('aria-hidden', 'false');
    detailPanel.inert = false;
    // Selecting another node inside the open inspector must retain its external initiator.
    if (!app.detailsOpen || !detailPanel.contains(document.activeElement)) {
        app.returnFocus = document.activeElement;
    }

    app.detailsOpen = true;
    document.querySelector('#detail-close').focus({ preventScroll: true });
    intro.classList.add('is-muted');
}

/** Hide details from pointer and keyboard navigation, restoring the initiating focus. */
function closeDetails() {
    app.dirty = true;
    app.selected = null;
    detailPanel.classList.remove('is-open');
    detailPanel.setAttribute('aria-hidden', 'true');
    detailPanel.inert = true;
    const wasOpen = app.detailsOpen;
    const initiator = app.returnFocus;
    app.detailsOpen = false;
    app.returnFocus = null;

    if (wasOpen) {
        // History can replace navigation while details are open; never focus detached or inert nodes.
        const target = initiator?.isConnected && !detailPanel.contains(initiator)
            ? initiator : document.querySelector('[data-layer="systems"]');
        target?.focus?.({ preventScroll: true });
    }
}

/** Draw stable labels and selected-node outlines over the graphics layer. */
function drawOverlay(visual, palette, time) {
    const context = labelCanvas.getContext('2d');
    const { width, height, dpr } = app.size;
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    context.clearRect(0, 0, width, height);
    context.lineCap = 'round';

    // Large field contours remain deliberately subtle; they make the two
    // namespaces legible without turning the layout into two isolated boxes.
    for (const node of app.domainNodes) {
        drawFieldContour(context, node.x, node.y, LOGICAL_WIDTH * .18, LOGICAL_HEIGHT * .14,
            nodeColor(node, palette), .08);
    }

    for (const item of visual.nodes) {
        const { node, x, y, size, color, alpha, selected, hovered } = item;
        if (alpha <= .02) continue;
        context.globalAlpha = alpha;
        if (selected || hovered) {
            context.strokeStyle = color;
            context.lineWidth = 1;
            context.beginPath();
            const pulse = app.running && !app.reducedMotion ? Math.sin(time * 2.5) * 2 : 0;
            context.arc(x, y, size * 1.35 + 6 + pulse, 0, TAU);
            context.stroke();
        }

        const shouldLabel = labelVisible(node, selected, hovered);
        if (app.size.width < 700 && !selected && !hovered) continue;
        if (!shouldLabel) continue;
        context.textAlign = 'center';
        context.textBaseline = 'middle';
        const project = node.kind === 'project' || node.kind === 'publication';
        const font = palette.mono;
        context.font = `${project ? 600 : 500} ${project ? 12 : 9}px ${font}`;
        context.fillStyle = palette.text;
        context.globalAlpha = Math.min(1, alpha * (selected || hovered ? 1.25 : .92));
        const label = project ? (node.surface_label ?? node.label) : node.label;
        const yOffset = size + (node.kind === 'project' || node.kind === 'publication' ? 20 : 13);
        context.fillText(label.toUpperCase(), x, y + yOffset);

        if ((node.kind === 'project' || node.kind === 'publication') && app.layer !== 'capabilities') {
            context.font = `8px ${palette.mono}`;
            context.fillStyle = palette.quiet;
            context.globalAlpha = alpha * .8;
            context.fillText(node.label.toUpperCase(), x, y + yOffset + 16);
        }
    }

    context.globalAlpha = 1;
}

/** Draw a subtle ownership contour in canonical coordinate space. */
function drawFieldContour(context, lx, ly, lrx, lry, color, alpha) {
    const center = logicalToScreen(lx, ly);
    const scale = app.size.fit * app.camera.zoom;
    context.globalAlpha = alpha * (app.domain === 'all' ? 1 : .55);
    context.strokeStyle = color;
    context.lineWidth = 1;
    context.setLineDash([2, 10]);
    context.beginPath();
    context.ellipse(center.x, center.y, lrx * scale, lry * scale, 0, 0, TAU);
    context.stroke();
    context.setLineDash([]);
}

/** Choose labels by layer while always labeling an explicit selection. */
function labelVisible(node, selected, hovered) {
    if (selected || hovered) return true;
    if (app.layer === 'overview') {
        return node.kind === 'project' || node.kind === 'publication' ||
            (node.kind === 'technology' && node.show_in_readme);
    }

    if (app.layer === 'systems') {
        return ['project', 'publication', 'component', 'package'].includes(node.kind);
    }

    return node.kind === 'technology' && (node.show_in_readme || app.camera.zoom > 1.35);
}

/** Use WebGL when available and retain the Canvas 2D rendering path. */
function createRenderer(canvas) {
    const gl = canvas.getContext('webgl2', { alpha: true, antialias: true, premultipliedAlpha: true });
    if (!gl) return new CanvasFallbackRenderer(canvas);

    return new WebGLRenderer(gl);
}

/** Render graph edges and glowing points in a single packed WebGL buffer. */
class WebGLRenderer {
    name = 'WEBGL2';
    /** Initialize backend-owned graphics resources. */
    constructor(gl) {
        this.gl = gl;
        this.program = makeProgram(gl, `#version 300 es
            in vec2 a_position;
            in vec4 a_color;
            in float a_size;
            out vec4 v_color;
            void main(){ gl_Position=vec4(a_position,0.,1.); gl_PointSize=a_size; v_color=a_color; }
        `, `#version 300 es
            precision highp float;
            in vec4 v_color;
            out vec4 outColor;
            uniform float u_points;
            void main(){
                if(u_points>.5){
                    vec2 c=gl_PointCoord-.5;
                    float d=length(c)*2.;
                    float core=1.-smoothstep(.18,1.,d);
                    float halo=(1.-smoothstep(.4,1.,d))*.36;
                    outColor=vec4(v_color.rgb,v_color.a*(core+halo));
                }else outColor=v_color;
            }

        `);
        this.locations = {
            position: gl.getAttribLocation(this.program, 'a_position'),
            color: gl.getAttribLocation(this.program, 'a_color'),
            size: gl.getAttribLocation(this.program, 'a_size'),
            points: gl.getUniformLocation(this.program, 'u_points'),
        };

        this.buffer = gl.createBuffer();
        this.vertexData = new Float32Array(0);
        this.colorCache = new Map();
        gl.enable(gl.BLEND);
        gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    }

    /** Theme values are supplied with each frame; no backend cache is required. */
    setTheme() { this.colorCache.clear(); }
    /** Update the backend viewport after a device or layout resize. */
    resize(size) { this.gl.viewport(0, 0, Math.round(size.width * size.dpr), Math.round(size.height * size.dpr)); }
    /** Draw a complete frame using reusable CPU and GPU capacity. */
    render(frame, palette, size) {
        const gl = this.gl;
        gl.clearColor(0, 0, 0, 0);
        gl.clear(gl.COLOR_BUFFER_BIT);
        gl.useProgram(this.program);
        const vertices = Math.max(frame.edges.length * 2, frame.nodes.length + frame.particles.length);
        this.reserve(vertices * 7);
        let offset = 0;

        for (const item of frame.edges) {
            offset = this.vertex(offset, item.from, item.color, item.alpha, 1, size);
            offset = this.vertex(offset, item.to, item.color, item.alpha, 1, size);
        }

        this.upload(offset);
        gl.uniform1f(this.locations.points, 0);
        gl.drawArrays(gl.LINES, 0, offset / 7);
        offset = 0;

        for (const items of [frame.nodes, frame.particles]) {
            for (const item of items) {
                offset = this.vertex(offset, item, item.color, item.alpha, Math.max(2, item.size * size.dpr), size);
            }
        }

        this.upload(offset);
        gl.uniform1f(this.locations.points, 1);
        gl.drawArrays(gl.POINTS, 0, offset / 7);
    }

    /** Grow only at graph changes; ordinary frames retain their backing storage. */
    reserve(length) {
        if (length <= this.vertexData.length) return;

        this.vertexData = new Float32Array(Math.max(length, this.vertexData.length * 2, 256));
        this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.buffer);
        this.gl.bufferData(this.gl.ARRAY_BUFFER, this.vertexData.byteLength, this.gl.DYNAMIC_DRAW);
    }

    /** Write one interleaved vertex without temporary coordinate/color arrays. */
    vertex(offset, point, color, alpha, radius, size) {
        let rgb = this.colorCache.get(color);

        if (!rgb) {
            rgb = cssColor(color);
            this.colorCache.set(color, rgb);
        }

        const data = this.vertexData;
        data[offset++] = point.x / size.width * 2 - 1;
        data[offset++] = 1 - point.y / size.height * 2;
        data[offset++] = rgb[0];
        data[offset++] = rgb[1];
        data[offset++] = rgb[2];
        data[offset++] = alpha;
        data[offset++] = radius;

        return offset;
    }

    /** Upload the used prefix without reallocating the GPU buffer or a JS view. */
    upload(length) {
        const gl = this.gl;
        const stride = 7 * 4;
        gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);

        if (length > 0) gl.bufferSubData(gl.ARRAY_BUFFER, 0, this.vertexData, 0, length);

        gl.enableVertexAttribArray(this.locations.position);
        gl.vertexAttribPointer(this.locations.position, 2, gl.FLOAT, false, stride, 0);
        gl.enableVertexAttribArray(this.locations.color);
        gl.vertexAttribPointer(this.locations.color, 4, gl.FLOAT, false, stride, 2 * 4);
        gl.enableVertexAttribArray(this.locations.size);
        gl.vertexAttribPointer(this.locations.size, 1, gl.FLOAT, false, stride, 6 * 4);
    }
}

/** Render the same visual frame on devices without WebGL. */
class CanvasFallbackRenderer {
    name = 'CANVAS2D';
    /** Initialize backend-owned graphics resources. */
    constructor(canvas) { this.context = canvas.getContext('2d'); }
    /** Theme values are supplied with each frame; no backend cache is required. */
    setTheme() {}
    /** Update the backend viewport after a device or layout resize. */
    resize() {}
    /** Draw a complete visual frame using the current palette and viewport. */
    render(frame, _palette, size) {
        const context = this.context;
        context.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
        context.clearRect(0, 0, size.width, size.height);
        context.lineCap = 'round';
        for (const item of frame.edges) {
            context.globalAlpha = item.alpha;
            context.strokeStyle = item.color;
            context.lineWidth = .8 + item.edge.weight;
            context.beginPath();
            context.moveTo(item.from.x, item.from.y);
            context.lineTo(item.to.x, item.to.y);
            context.stroke();
        }

        for (const item of [...frame.nodes, ...frame.particles]) {
            context.globalAlpha = item.alpha;
            context.fillStyle = item.color;
            context.shadowColor = item.color;
            context.shadowBlur = Math.min(18, item.size * .5);
            context.beginPath(); context.arc(item.x, item.y, Math.max(1.5, item.size * .28), 0, TAU); context.fill();
        }

        context.shadowBlur = 0;
        context.globalAlpha = 1;
    }
}

/** Compile and link the minimal point-and-line shader program. */
function makeProgram(gl, vertexSource, fragmentSource) {
    const compile = (type, source) => {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));

        return shader;
    };

    const program = gl.createProgram();
    gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));

    return program;
}

/** Capture CSS tokens at startup and on theme changes; animation frames reuse this snapshot. */
function readPalette() {
    const style = getComputedStyle(document.documentElement);
    const value = name => style.getPropertyValue(name).trim();

    return {
        mono: value('--mono'), text: value('--text'), muted: value('--muted'), quiet: value('--quiet'),
        personal: value('--personal'), personal2: value('--personal-2'),
        organization: value('--organization'), organization2: value('--organization-2'), shared: value('--shared'),
    };
}

/** Convert a CSS hexadecimal theme color to normalized RGBA values. */
function cssColor(value, alpha = 1) {
    const hex = value.replace('#', '');
    const number = Number.parseInt(hex.length === 3 ? hex.split('').map(c => c + c).join('') : hex, 16);

    return [((number >> 16) & 255) / 255, ((number >> 8) & 255) / 255, (number & 255) / 255, alpha];
}

/** Derive stable particle phases from semantic identity. */
function seededFraction(hash, index) {
    let value = (Number.parseInt(hash.slice(0, 8), 16) ^ Math.imul(index, 0x9e3779b9)) >>> 0;
    value ^= value << 13; value ^= value >>> 17; value ^= value << 5;

    return (value >>> 0) % 10000 / 10000;
}

/** Group the state digest for human scanning. */
function formatHash(value) { return value.match(/.{1,4}/g)?.join('\u00b7') ?? value; }
/** Constrain a numeric value to inclusive bounds. */
function clamp(value, min, max) { return Math.max(min, Math.min(max, value)); }
/** Interpolate a particle position along an edge. */
function lerp(from, to, t) { return from + (to - from) * t; }
/** Accept public HTTPS links without credentials or executable URL schemes. */
function safePublicUrl(value) {
    try {
        const url = new URL(value);

        return url.protocol === 'https:' && !url.username && !url.password;
    } catch {
        return false;
    }
}

/** Update coordinates on every gesture boundary; taps need no preceding move. */
function updatePointer(event) {
    app.dirty = true;
    const rect = stage.getBoundingClientRect();
    app.pointer.x = event.clientX - rect.left;
    app.pointer.y = event.clientY - rect.top;
}

/** Let native controls and the scrollable profile own their input events. */
function ignoreGesture(event) {
    return app.layer === 'overview' || Boolean(event.target.closest('button, a, select, input, aside'));
}

/** Canceling a gesture must not activate its last hit target. */
function cancelPointer() {
    app.pointer.down = false;
    app.pointer.moved = true;
    app.pointer.id = null;
    stage.classList.remove('is-dragging');
}

/** Render a keyboard and narrow-screen alternative to the spatial graph. */
function renderNavigation() {
    const list = document.querySelector('#node-list');
    list.replaceChildren();
    const nodes = app.nodes.filter(node => node.kind !== 'domain' &&
        (app.domain === 'all' || node.domain === app.domain || node.domain == null));

    for (const node of nodes) {
        const item = document.createElement('li');
        const button = document.createElement('button');
        button.type = 'button';
        button.textContent = node.label;
        button.dataset.nodeId = node.id;
        button.addEventListener('click', () => openDetails(node));
        const summary = document.createElement('p');
        summary.textContent = node.summary ?? '';
        const stack = document.createElement('small');
        stack.textContent = (node.display_stack ?? []).join(' / ');
        item.append(button, summary, stack);
        list.append(item);
    }

    const presentation = app.state.presentation ?? {};
    const context = document.querySelector('#profile-context');
    context.replaceChildren();
    const groups = app.state.profile.variant === 'organization' ? [] : [
        ['Languages and tools', [...(presentation.main_stack ?? []), ...(presentation.supporting_stack ?? [])]],
        ['Interests', (app.state.interests ?? []).map(value => value.label)],
        ['Learning', (app.state.learning ?? []).map(value => value.label)],
        ['Platforms', presentation.platforms ?? []],
        ['Hardware', (presentation.hardware ?? []).map(value => `${value.label}: ${value.detail}`)],
    ];

    for (const [label, values] of groups) {
        if (!values.length) continue;

        const heading = document.createElement('h3');
        heading.textContent = label;
        const text = document.createElement('p');
        text.textContent = values.join(' / ');
        context.append(heading, text);
    }

    for (const note of app.state.profile.variant === 'organization' ? [] : [presentation.platform_note, presentation.learning_note]) {
        if (!note) continue;

        const text = document.createElement('p');
        text.textContent = note;
        context.append(text);
    }

    for (const membership of app.state.profile.variant === 'organization' ? [] : app.state.learning ?? []) {
        if (!safePublicUrl(membership.url)) continue;

        const link = document.createElement('a');
        link.href = membership.url;
        link.rel = 'noreferrer';
        link.textContent = membership.label;
        const paragraph = document.createElement('p');
        paragraph.append(link);
        context.append(paragraph);
    }
}

/** Reject active SVG content before attaching a same-origin generated artifact. */
function parseProfileSvg(source) {
    // Chromium evaluates SVG style policy during XML parsing, before the detached tree is imported.
    // The browser owns these animations in its external stylesheet, so remove the generated CSS first.
    const retained = [];
    let cursor = 0;
    let styleOpen = false;

    for (const match of source.matchAll(/<style(?:\s[^<>]*)?>|<\/style\s*>/gi)) {
        if (match[0].startsWith('</')) {
            if (!styleOpen) throw new Error('Invalid profile SVG');

            cursor = match.index + match[0].length;
            styleOpen = false;
        } else {
            if (styleOpen) throw new Error('Invalid profile SVG');

            retained.push(source.slice(cursor, match.index));
            styleOpen = true;
        }
    }

    if (styleOpen) throw new Error('Invalid profile SVG');

    retained.push(source.slice(cursor));
    const markup = retained.join('');

    // Reject style syntax left or recreated by deletion, including qualified XML names.
    // DTD expansion and stylesheet instructions must not bypass this pre-parse boundary.
    if (/<\/?(?:[^<>\s]*:)?style(?:[\s/>]|$)/i.test(markup) ||
        /<!DOCTYPE|<\?xml-stylesheet\b/i.test(markup)) {
        throw new Error('Invalid profile SVG');
    }

    // Attribute styles also trigger CSP during XML parsing; quoted values are not attribute names.
    for (const tag of markup.matchAll(/<(?![/?!\s])(?:[^<>"']|"[^"<]*"|'[^'<]*')*>/g)) {
        for (const attribute of tag[0].matchAll(/"[^"]*"|'[^']*'|(\s(?:[^\s"'=<>]*:)?style\s*=)/gi)) {
            if (attribute[1]) throw new Error('Invalid profile SVG');
        }
    }

    const parsed = new DOMParser().parseFromString(markup, 'image/svg+xml');
    const svg = parsed.documentElement;

    const activeElements = 'parsererror, script, foreignObject, iframe, image, use, animate, animateTransform, set';

    if (svg.localName !== 'svg' || parsed.querySelector(activeElements)) {
        throw new Error('Invalid profile SVG');
    }

    for (const element of [svg, ...svg.querySelectorAll('*')]) {
        if (element.localName === 'style') {
            element.remove();
            continue;
        }

        for (const attribute of [...element.attributes]) {
            const name = attribute.localName.toLowerCase();
            const value = attribute.value;

            if (name.startsWith('on') || name === 'style') {
                element.removeAttributeNode(attribute);
            } else if (name === 'href' && !value.startsWith('#') && !safePublicUrl(value)) {
                throw new Error('Invalid profile link');
            } else if (/url\s*\(/i.test(value) && !/^url\(#[\w-]+\)$/.test(value)) {
                throw new Error('Invalid profile resource');
            }
        }
    }

    return document.importNode(svg, true);
}

/** Restore timing whenever visibility or motion policy may have recreated CSS effects. */
function syncProfileAnimations() {
    const svg = document.querySelector('#profile-view svg');

    if (!svg || !app.state || app.reducedMotion) return;

    // CSS destroys effects for display:none and reduced motion. Query after layout or pause updates
    // so fresh effects inherit the same timing when themes, layers, or responsive visibility change.
    configureProfileAnimations(svg, Math.max(12, app.state.canvas.motion_seconds));
}

/** Apply numeric timing after sanitization without accepting inline SVG style declarations. */
function configureProfileAnimations(svg, baseSeconds) {
    for (const animation of svg.getAnimations({ subtree: true })) {
        const target = animation.effect.target;
        let duration;

        switch (animation.animationName) {
            case 'orbit':
                duration = target.classList.contains('scan') ? 24 : baseSeconds + 26;
                break;
            case 'flow':
                duration = baseSeconds;
                break;
            case 'pulse':
                duration = 6;
                break;
            case 'icon-signal': {
                const phase = target.classList.contains('icon-signal-phase-2') ? 2 :
                    target.classList.contains('icon-signal-phase-1') ? 1 : 0;

                animation.effect.updateTiming({ duration: 5400, delay: phase === 0 ? 0 : -1800 * phase });
                continue;
            }

            case 'signal': {
                const value = target.dataset.signalDelay ?? '';
                const delay = /^-?(?:\d+(?:\.\d+)?|\.\d+)$/.test(value) ? Number(value) * 1000 : NaN;

                // Convert only finite nonpositive seconds to API numbers; untrusted CSS never reaches a style sink.
                animation.effect.updateTiming({
                    duration: 3600,
                    delay: Number.isFinite(delay) && delay <= 0 ? delay : 0,
                });

                continue;
            }

            default:
                continue;
        }

        animation.effect.updateTiming({ duration: duration * 1000 });
    }
}

/** Load the canonical renderer rather than maintaining a second factual layout. */
async function loadProfile() {
    const host = document.querySelector('#profile-view');
    if (!app.renderer) return;

    try {
        const response = await fetch(`./sourcefield.${mediaLight.matches ? 'light' : 'dark'}.svg`);

        if (!response.ok) throw new Error('Profile unavailable');

        const svg = parseProfileSvg(await response.text());
        // The semantic list remains the complete accessible representation of the graphic.
        svg.setAttribute('role', 'group');
        svg.setAttribute('aria-label', 'Project field. Use the project list for details.');
        host.replaceChildren(svg);
        for (const element of svg.querySelectorAll('[data-node-id]')) {
            const node = app.nodeById.get(element.dataset.nodeId)?.node;

            if (!node || element.localName === 'a' || element.querySelector('a')) continue;

            element.setAttribute('role', 'button');
            element.setAttribute('tabindex', '0');
            element.setAttribute('aria-label', `Details: ${node.label}`);
            element.addEventListener('click', () => openDetails(node));
            element.addEventListener('keydown', event => {
                if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    openDetails(node);
                }
            });
        }

        updateProfileFilter();
        updateMotionButton();
    } catch {
        host.textContent = 'The field image is unavailable. All project details are available below.';
    }

    updateLayer();
}

/** Apply namespace selection equally to spatial and semantic presentations. */
function updateProfileFilter() {
    for (const element of document.querySelectorAll('#profile-view [data-domain]')) {
        element.classList.toggle('is-filtered', app.domain !== 'all' && element.dataset.domain !== app.domain);
    }
}

/** Keep the approved profile primary and the exploratory graph an explicit layer. */
function updateLayer() {
    app.dirty = true;
    const overview = app.layer === 'overview';
    if (window.matchMedia('(max-width: 700px)').matches) {
        document.querySelector('.semantic-profile details').open = true;
    }

    stage.classList.toggle('profile-mode', overview);
    document.querySelector('#profile-view').hidden = !overview;
    document.querySelector('#gesture-hint').hidden = overview;
    intro.hidden = true;
    glCanvas.hidden = overview;
    labelCanvas.hidden = overview;
    document.querySelector('.stage-vignette').hidden = overview;

    for (const button of document.querySelectorAll('[data-layer]')) {
        button.setAttribute('aria-pressed', String(button.dataset.layer === app.layer));
    }

    resize();
}

/** Validate the complete producer protocol before publishing any selectable history entries. */
function validatedHistoryEntries(index) {
    if (index?.schema_version !== 3 || !Array.isArray(index.states) || index.states.length > 256) {
        throw new Error('Unsupported history format or capacity; run the explicit migration tool');
    }

    const identities = new Set();

    for (const entry of index.states) {
        if (typeof entry?.hash !== 'string' || !/^[a-f0-9]{16,64}$/i.test(entry.hash)) {
            throw new Error('Invalid history hash');
        }

        // Native semantic identifiers are uppercase; archive filenames are canonical lowercase.
        const identity = entry.hash.toLowerCase();

        if (entry.file !== `${identity}.json` || identities.has(identity)) {
            throw new Error('Invalid history filename or duplicate identity');
        }

        identities.add(identity);
    }

    return index.states;
}

/** Load only validated local snapshot names; history never redirects fetches. */
async function loadHistory() {
    const select = document.querySelector('#history-select');
    const files = new Map();

    try {
        const response = await fetch('./history/index.json');

        if (!response.ok) return;

        const entries = validatedHistoryEntries(await response.json());

        for (const entry of entries) {
            const hash = entry.hash.toLowerCase();
            files.set(hash, entry.file);

            const option = document.createElement('option');
            option.value = hash.toLowerCase();
            option.textContent = `${entry.generated_at ?? 'Snapshot'} / ${hash.slice(0, 8)}`;
            select.append(option);
        }
    } catch {
        // An absent archive does not invalidate the current profile.
        select.disabled = true;
    }

    let request = 0;

    select.addEventListener('change', async () => {
        const currentRequest = ++request;
        const selected = select.value;

        try {
            if (selected && !files.has(selected)) throw new Error('Unknown history selection');

            const path = selected ? `./history/${files.get(selected)}` : './profile-state.json';
            const response = await fetch(path);

            if (!response.ok) throw new Error('Snapshot unavailable');

            const state = validateState(await response.json());
            if (selected && state.semantic_hash.toLowerCase() !== selected) throw new Error('Snapshot identity mismatch');

            const simulator = await createSimulator(state, state.nodes, state.edges);

            if (currentRequest !== request) {
                simulator.free?.();
                return;
            }

            document.querySelector('[data-layer="overview"]').disabled = Boolean(select.value);
            app.simulator.free?.();
            app.simulator = simulator;
            app.engine = simulator.engineLabel;
            app.state = state;
            configureIdentity(state);
            LOGICAL_WIDTH = state.canvas.width;
            LOGICAL_HEIGHT = state.canvas.height;
            app.nodes = state.nodes;
            app.edges = state.edges;
            app.nodeById = new Map(app.nodes.map((node, i) => [node.id, { node, index: i }]));
            app.positions = new Float32Array(app.nodes.flatMap(node => [node.x, node.y]));

            app.layer = 'systems';
            document.querySelectorAll('[data-layer]').forEach(button =>
                button.classList.toggle('is-active', button.dataset.layer === app.layer));
            stateLabel.textContent = `STATE ${formatHash(state.semantic_hash)} / ${state.mode.toUpperCase()}`;
            engineLabel.textContent = `${app.engine} / ${app.renderer.name}`;
            renderMetrics();
            renderNavigation();
            resetView();
            updateLayer();
        } catch {
            if (currentRequest !== request) return;

            stateLabel.textContent = 'SNAPSHOT UNAVAILABLE';
        }
    });
}


/** Reject invalid snapshot geometry before replacing the active engine or graph. */
function validateState(state) {
    if (!state || state.schema_version !== 3 || !Array.isArray(state.nodes) || !Array.isArray(state.edges) || !state.canvas ||
        !state.profile || !['personal', 'organization'].includes(state.profile.variant) ||
        !state.stats || typeof state.mode !== 'string' || state.nodes.length > 512 || state.edges.length > 8192 ||
        !/^[a-f0-9]{16,64}$/i.test(state.semantic_hash ?? '')) {
        throw new Error('Invalid state');
    }

    if (state.profile.variant === 'organization' && (
        (state.interests?.length ?? 0) > 0 || (state.learning?.length ?? 0) > 0 ||
        (state.presentation?.hardware?.length ?? 0) > 0 || (state.presentation?.platforms?.length ?? 0) > 0 ||
        state.stats.private_repository_count != null ||
        state.nodes.some(node => node.kind === 'interest' ||
            (node.kind === 'domain' && node.scope !== 'organization')))) {
        throw new Error('Organization state contains personal content');
    }

    const { width, height } = state.canvas;

    if (![width, height].every(value => Number.isFinite(value) && value > 0 && value <= 100000)) {
        throw new Error('Invalid canvas');
    }

    const identifiers = new Set();

    for (const node of state.nodes) {
        if (!node || typeof node.id !== 'string' || identifiers.has(node.id) ||
            typeof node.label !== 'string' || !Number.isFinite(node.x) || !Number.isFinite(node.y) ||
            node.x < 0 || node.y < 0 || node.x > width || node.y > height) {
            throw new Error('Invalid node');
        }

        identifiers.add(node.id);
    }

    if (!state.nodes.length || state.edges.some(edge => !identifiers.has(edge.from) ||
        !identifiers.has(edge.to) || !Number.isFinite(edge.weight) || edge.weight < 0 || edge.weight > 1)) {
        throw new Error('Invalid edge');
    }

    return state;
}


boot().catch(() => {
    engineLabel.textContent = 'FIELD INITIALIZATION FAILED';
    stateLabel.textContent = 'PROFILE DATA UNAVAILABLE';
});
