// Pick a reasonable Vega-Lite chart for a result's columns, or null to fall back
// to a plain table. Mirrors the design heuristic; used as the AI fallback and as
// the manual "auto-chart" default. See docs/design/dives.md.

import type { Column } from '../pipeline-types';
import type { DiveChart } from './dive-types';

type Role = 'temporal' | 'quantitative' | 'nominal';

function roleOf(type: unknown): Role {
    const t = String(type ?? '').toLowerCase();
    if (/(date|time|timestamp)/.test(t)) return 'temporal';
    if (/(int|float|double|decimal|numeric|real|big|huge|tiny|small)/.test(t)) return 'quantitative';
    return 'nominal';
}

function chart(mark: string, xField: string, xType: Role, yField: string): DiveChart {
    return {
        mark: mark === 'point' ? { type: 'point', filled: true } : mark,
        encoding: {
            x: { field: xField, type: xType },
            y: { field: yField, type: 'quantitative' },
        },
    };
}

export function suggestChart(columns: Column[]): DiveChart | null {
    const cols = columns.map((c) => ({ name: c.name, role: roleOf(c.type) }));
    const temporal = cols.filter((c) => c.role === 'temporal');
    const quant = cols.filter((c) => c.role === 'quantitative');
    const nominal = cols.filter((c) => c.role === 'nominal');

    // time + measure -> line; category + measure -> bar; two measures -> scatter.
    if (temporal.length >= 1 && quant.length >= 1) {
        return chart('line', temporal[0].name, 'temporal', quant[0].name);
    }
    if (nominal.length >= 1 && quant.length >= 1) {
        return chart('bar', nominal[0].name, 'nominal', quant[0].name);
    }
    if (quant.length >= 2) {
        return chart('point', quant[0].name, 'quantitative', quant[1].name);
    }
    return null; // nothing sensible to chart -> render the table
}
