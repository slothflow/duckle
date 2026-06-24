// Renders a dive's Vega-Lite spec. Data is bound as a NAMED dataset, so when the
// rows change (a re-query) we rebind without a full re-embed - that is what makes
// a dive stay never-stale cheaply. vega-embed is lazy-imported so the editor hot
// path pays nothing until a dive opens. See docs/design/dives.md.

import { useEffect, useRef } from 'react';
import type { Result, VisualizationSpec } from 'vega-embed';

interface VegaChartProps {
    spec: Record<string, unknown>;
    rows: Record<string, unknown>[];
    theme?: 'light' | 'dark';
}

const DATASET = 'dive';

/** Brand-token Vega config (lemon/orange/maya/slate; success = maya, no green). */
function vegaConfig(theme: 'light' | 'dark') {
    const ink = theme === 'dark' ? '#ecf0f7' : '#1b2030';
    const grid = theme === 'dark' ? 'rgba(255,255,255,0.08)' : 'rgba(0,0,0,0.08)';
    return {
        background: 'transparent',
        range: { category: ['#ffd84d', '#ff7a45', '#2eafff', '#ed5f22', '#aab3c5'] },
        axis: { labelColor: ink, titleColor: ink, gridColor: grid, domainColor: grid, tickColor: grid },
        legend: { labelColor: ink, titleColor: ink },
        title: { color: ink },
        view: { stroke: 'transparent' },
    };
}

export function VegaChart({ spec, rows, theme = 'dark' }: VegaChartProps) {
    const elRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<Result['view'] | null>(null);
    const specKey = JSON.stringify(spec);

    // (Re)embed only when the spec or theme changes.
    useEffect(() => {
        const el = elRef.current;
        if (!el) return;
        let cancelled = false;
        let view: Result['view'] | null = null;
        void (async () => {
            try {
                const { default: embed } = await import('vega-embed');
                const full = { ...spec, data: { name: DATASET }, datasets: { [DATASET]: rows } };
                const res = await embed(el, full as unknown as VisualizationSpec, {
                    actions: false,
                    renderer: 'canvas',
                    config: vegaConfig(theme),
                });
                if (cancelled) {
                    res.finalize();
                    return;
                }
                view = res.view;
                viewRef.current = res.view;
            } catch (e) {
                if (el) el.textContent = `Chart error: ${String(e)}`;
            }
        })();
        return () => {
            cancelled = true;
            if (view) view.finalize();
            viewRef.current = null;
        };
        // rows are rebound by the effect below; re-embedding on every row change
        // would throw away the chart state.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [specKey, theme]);

    // Rebind data in place when only the rows change.
    useEffect(() => {
        const v = viewRef.current;
        if (!v) return;
        v.data(DATASET, rows);
        void v.runAsync();
    }, [rows]);

    return <div ref={elRef} className="dive-chart" />;
}
