import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import { isWebBackend } from './web-fs';

// First-run guided tour: a spotlight walkthrough of the core surfaces. Anchors
// to [data-tour="..."] markers; if a marker is missing the step degrades to a
// centered card, so the tour never breaks. Dismissal persists to localStorage;
// re-launch by dispatching window event "duckle:start-tour".

// Bumped to v3: the tour is now surface-aware (a step that targets a button only
// the desktop app shows is dropped on the self-hosted web editor, so the step
// count and spotlights always match what is on screen), and gained Save,
// Run-parameters and Trust coverage plus richer how-to copy.
// Bumped to v4: added a Live preview step (the lightning toggle is otherwise
// easy to miss), so prior users who finished v3 see it once.
const SEEN_KEY = 'duckle.tour.v4.done';

type Placement = 'top' | 'bottom' | 'left' | 'right' | 'center';
// 'both' shows everywhere; 'desktop' only in the Tauri app; 'web' only in the
// self-hosted web editor. Undefined is treated as 'both'.
type Surface = 'both' | 'desktop' | 'web';
interface Step {
    sel: string | null;
    title: string;
    body: string;
    placement?: Placement;
    surface?: Surface;
}

const ALL_STEPS: Step[] = [
    {
        sel: null,
        title: 'Welcome to Duckle',
        body: 'Your local-first DuckDB studio for building data pipelines - no servers, no JVM, your data never leaves the machine. This 60-second tour points out the essentials. You can skip it now and replay it later from Settings.',
        placement: 'center',
    },
    {
        sel: '[data-tour="palette"]',
        title: 'Palette & project tree',
        body: 'This left panel has two tabs. The Palette holds 350+ building blocks - databases, files, cloud + object stores, vector DBs, data-quality & governance, AI, and code UDFs (Python, JavaScript, SQL). Drag one onto the canvas, or just start typing on the canvas to quick-add by name. The Project tab browses your pipelines, saved connections and bundled examples.',
        placement: 'right',
    },
    {
        sel: '[data-tour="canvas"]',
        title: 'Pipeline canvas',
        body: 'Wire blocks together to build a pipeline: drag from a node output port to the next node input. A typical flow is source to transform to sink. Right-click a node for actions like Run to here, and drag on empty space to pan.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="properties"]',
        title: 'Properties',
        body: 'Select any node to configure it here: connection details, the SQL or query, columns, and write modes (overwrite / append / upsert). Use ${name} placeholders for values you want to fill in per environment or per run.',
        placement: 'left',
    },
    {
        sel: '[data-tour="save"]',
        title: 'Save your work',
        body: 'Save the pipeline to your workspace (Ctrl+S also works). Pipelines are plain JSON files on disk, so they version-control cleanly and you can hand-edit or diff them.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="run"]',
        title: 'Run your pipeline',
        body: 'Run the pipeline locally with DuckDB. The data preview, the compiled execution plan, and run logs appear in the panel below the canvas. You can also Run to here from a node right-click menu. If the pipeline uses ${...} variables that no context fills, a small dialog asks for their values first - so the same pipeline can process, say, a specific month on demand.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="live"]',
        title: 'Live preview',
        body: 'Click this lightning toggle to turn on live preview. With it on, editing any node\'s settings automatically re-runs the pipeline up to that node and refreshes its Preview tab - so you see the resulting rows update as you tweak, without pressing Run each time. Open the edited node\'s Preview tab to watch it. It stays quiet while the pipeline has validation errors or a run is already in progress; toggle it off to edit without re-running.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="dashboard"]',
        title: 'Web dashboard',
        body: 'Open the management console in your browser: run and monitor every pipeline grouped by job, set its run parameters, and see schedules and full run history. (This is the console that duckle-runner serve hosts.)',
        placement: 'bottom',
        surface: 'desktop',
    },
    {
        sel: '[data-tour="dives"]',
        title: 'Dives - live data views & dashboards',
        body: 'Explore your data with live, auto-charting views, then pin them into shareable dashboards - all local-first. A fast way to inspect results without leaving Duckle. (You can hide this button from Settings.)',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="lineage"]',
        title: 'Column lineage',
        body: 'Trace any output column back through every transform to the source columns it came from - handy for audits and impact analysis before you change a query.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="trust"]',
        title: 'Trust score & report',
        body: 'See a trust report for the pipeline: a signed run manifest, source input hashes, and schema-drift detection that flags when an upstream source columns or types change since the last signed run. Use it to gate a pipeline as review-ready.',
        placement: 'bottom',
    },
    {
        sel: '[data-tour="topbar"]',
        title: 'AI, Git, context & settings',
        body: 'From the top bar you can connect an AI assistant over MCP, use built-in Git version control, switch your context / environment (the values behind those ${...} placeholders), change language, and open Settings - where you can also replay this tour.',
        placement: 'bottom',
    },
    {
        sel: null,
        title: "You're all set",
        body: 'Build your first pipeline, or open one of the bundled examples from the Project tab. You can replay this tour anytime from Settings.',
        placement: 'center',
    },
];

// Keep only the steps that apply to the current surface, so the step count and
// the spotlights always match what is actually on screen. The desktop-only
// dashboard button, for example, is not rendered in the web editor, so its step
// is dropped there rather than degrading to an anchorless centered card.
const onWeb = isWebBackend();
const STEPS: Step[] = ALL_STEPS.filter((s) => {
    const surface = s.surface ?? 'both';
    return surface === 'both' || (surface === 'desktop' && !onWeb) || (surface === 'web' && onWeb);
});

interface Box {
    top: number;
    left: number;
    width: number;
    height: number;
}

export function GuidedTour() {
    const [active, setActive] = useState(false);
    const [i, setI] = useState(0);
    const [box, setBox] = useState<Box | null>(null);

    // Open on first run - but only once the workspace UI is actually mounted
    // (poll for the canvas anchor), so brand-new users still on the engine-setup
    // screen don't see a tour pointing at elements that don't exist yet.
    useEffect(() => {
        if (localStorage.getItem(SEEN_KEY)) return;
        let tries = 0;
        const iv = setInterval(() => {
            tries += 1;
            if (document.querySelector('[data-tour="canvas"]')) {
                clearInterval(iv);
                setActive(true);
            } else if (tries > 40) {
                clearInterval(iv);
            }
        }, 600);
        return () => clearInterval(iv);
    }, []);
    useEffect(() => {
        const start = () => {
            setI(0);
            setActive(true);
        };
        window.addEventListener('duckle:start-tour', start);
        return () => window.removeEventListener('duckle:start-tour', start);
    }, []);

    const measure = useCallback(() => {
        const step = STEPS[i];
        if (!step?.sel) {
            setBox(null);
            return;
        }
        const el = document.querySelector(step.sel) as HTMLElement | null;
        if (!el) {
            setBox(null);
            return;
        }
        const r = el.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) {
            setBox(null);
            return;
        }
        setBox({ top: r.top, left: r.left, width: r.width, height: r.height });
    }, [i]);

    useLayoutEffect(() => {
        if (!active) return;
        measure();
        window.addEventListener('resize', measure);
        window.addEventListener('scroll', measure, true);
        return () => {
            window.removeEventListener('resize', measure);
            window.removeEventListener('scroll', measure, true);
        };
    }, [active, measure]);

    if (!active) return null;

    const step = STEPS[i];
    const last = i === STEPS.length - 1;
    const close = () => {
        localStorage.setItem(SEEN_KEY, '1');
        setActive(false);
    };
    const next = () => (last ? close() : setI((n) => n + 1));
    const back = () => setI((n) => Math.max(0, n - 1));

    // Tooltip position: anchored beside the spotlight, then ALWAYS clamped into
    // the viewport so the card (and its Skip/Back/Next buttons) is reachable even
    // when the target fills the screen (e.g. the canvas). Very large targets get
    // a centered card since "beside" has no room.
    const PAD = 10;
    const TIP_W = 340;
    const TIP_H = 280; // generous estimate used only for clamping
    const vh = window.innerHeight;
    const vw = window.innerWidth;
    let tipStyle: React.CSSProperties;
    const big = !!box && box.height > vh * 0.7 && box.width > vw * 0.45;
    if (!box || big) {
        tipStyle = { top: '50%', left: '50%', transform: 'translate(-50%,-50%)' };
    } else {
        const place = step.placement ?? 'bottom';
        let top: number;
        let left: number;
        if (place === 'right' && box.left + box.width + TIP_W + 24 < vw) {
            left = box.left + box.width + PAD;
            top = box.top;
        } else if (place === 'left' && box.left - TIP_W - 24 > 0) {
            left = box.left - TIP_W - PAD;
            top = box.top;
        } else if (place === 'top' && box.top - TIP_H - PAD > 0) {
            top = box.top - TIP_H - PAD;
            left = box.left;
        } else {
            // bottom (default); if it would overflow, flip above the target
            top = box.top + box.height + PAD;
            left = box.left;
            if (top + TIP_H + 12 > vh && box.top - TIP_H - PAD > 0) {
                top = box.top - TIP_H - PAD;
            }
        }
        // Final guard: keep the whole card on screen.
        top = Math.max(12, Math.min(top, vh - TIP_H - 12));
        left = Math.max(12, Math.min(left, vw - TIP_W - 12));
        tipStyle = { top, left };
    }

    return (
        <div className="tour-root" role="dialog" aria-modal="true" aria-label="Duckle guided tour">
            {/* Spotlight: a transparent box with a huge shadow dims everything else. */}
            {box ? (
                <div
                    className="tour-spotlight"
                    style={{
                        top: box.top - PAD,
                        left: box.left - PAD,
                        width: box.width + PAD * 2,
                        height: box.height + PAD * 2,
                    }}
                />
            ) : (
                <div className="tour-dim" onClick={close} />
            )}
            <div className="tour-tip" style={{ ...tipStyle, width: TIP_W }}>
                <div className="tour-progress">
                    Step {i + 1} of {STEPS.length}
                </div>
                <h3 className="tour-title">{step.title}</h3>
                <p className="tour-body">{step.body}</p>
                <div className="tour-dots">
                    {STEPS.map((_, n) => (
                        <span key={n} className={n === i ? 'tour-dot on' : 'tour-dot'} />
                    ))}
                </div>
                <div className="tour-actions">
                    <button type="button" className="tour-skip" onClick={close}>
                        Skip tour
                    </button>
                    <div className="tour-nav">
                        {i > 0 ? (
                            <button type="button" className="tour-btn" onClick={back}>
                                Back
                            </button>
                        ) : null}
                        <button type="button" className="tour-btn primary" onClick={next}>
                            {last ? 'Get started' : 'Next'}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}
