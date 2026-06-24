// Opens a dive: runs its query (never-stale) and renders the chart plus the
// underlying-data table. If the chart's encoding references a column the query
// did not return, fall back to the table only rather than rendering a broken
// chart. See docs/design/dives.md.

import { useEffect, useState } from 'react';
import type { Dive } from './dive-types';
import { runDive, type DiveResult } from './dive-run';
import { VegaChart } from './VegaChart';

interface DivePanelProps {
    dive: Dive;
    workspacePath?: string | null;
    theme?: 'light' | 'dark';
}

/** Field names referenced by a Vega-Lite spec's encoding channels. */
function encodingFields(spec: Record<string, unknown>): string[] {
    const enc = (spec.encoding ?? {}) as Record<string, unknown>;
    const fields: string[] = [];
    for (const channel of Object.values(enc)) {
        const f = (channel as Record<string, unknown> | null)?.field;
        if (typeof f === 'string') fields.push(f);
    }
    return fields;
}

export function DivePanel({ dive, workspacePath, theme = 'dark' }: DivePanelProps) {
    const [result, setResult] = useState<DiveResult | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        let cancelled = false;
        setLoading(true);
        setError(null);
        runDive(dive, workspacePath)
            .then((r) => {
                if (!cancelled) {
                    setResult(r);
                    setLoading(false);
                }
            })
            .catch((e: unknown) => {
                if (!cancelled) {
                    setError(e instanceof Error ? e.message : String(e));
                    setLoading(false);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [dive, workspacePath]);

    if (loading) return <div className="dive-panel-msg">Running query...</div>;
    if (error) return <div className="dive-panel-msg dive-panel-err">{error}</div>;
    if (!result) return null;

    const cols = result.columns.map((c) => c.name);
    const fields = encodingFields(dive.chart);
    const chartRenderable = fields.length > 0 && fields.every((f) => cols.includes(f));

    return (
        <div className="dive-panel">
            <div className="dive-panel-head">{dive.title}</div>
            {chartRenderable ? <VegaChart spec={dive.chart} rows={result.rows} theme={theme} /> : null}
            <DiveTable columns={cols} rows={result.rows} />
        </div>
    );
}

function fmt(v: unknown): string {
    if (v === null || v === undefined) return '';
    if (typeof v === 'object') return JSON.stringify(v);
    return String(v);
}

function DiveTable({ columns, rows }: { columns: string[]; rows: Record<string, unknown>[] }) {
    const shown = rows.slice(0, 100);
    return (
        <div className="dive-table-wrap">
            <table className="dive-table">
                <thead>
                    <tr>
                        {columns.map((c) => (
                            <th key={c}>{c}</th>
                        ))}
                    </tr>
                </thead>
                <tbody>
                    {shown.map((row, i) => (
                        <tr key={i}>
                            {columns.map((c) => (
                                <td key={c}>{fmt(row[c])}</td>
                            ))}
                        </tr>
                    ))}
                </tbody>
            </table>
            {rows.length > shown.length ? (
                <div className="dive-table-more">{rows.length - shown.length} more rows</div>
            ) : null}
        </div>
    );
}
