// AI text-to-dive: describe the source to ground Duckie, ask for {sql, chart},
// and parse the reply. Rides the existing chat_send bridge (desktop bundled Qwen,
// or an external endpoint via #92) - no new backend command. The generated SQL is
// always previewed (run) before save, which is the validation loop in v1.
// See docs/design/dives.md.

import type { Column } from '../pipeline-types';
import { chatSend, type ChatMessage } from '../tauri-bridge';
import { runDive } from './dive-run';
import type { DiveChart } from './dive-types';

export interface GeneratedDive {
    sql: string;
    chart: DiveChart;
}

function schemaCard(columns: Column[], rows: Record<string, unknown>[]): string {
    return columns
        .slice(0, 60)
        .map((c) => {
            const sample = rows.find((r) => r[c.name] != null)?.[c.name];
            const eg = sample === undefined ? '' : ` e.g. ${String(sample).slice(0, 24)}`;
            return `  - ${c.name} (${String(c.type)})${eg}`;
        })
        .join('\n');
}

/** Pull { sql, chart } from a fenced ```json block, or the first {...}. */
export function parseGenerated(text: string): GeneratedDive | null {
    const fence = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
    const open = text.indexOf('{');
    const close = text.lastIndexOf('}');
    const raw = fence ? fence[1] : open >= 0 && close > open ? text.slice(open, close + 1) : '';
    if (!raw.trim()) return null;
    try {
        const o = JSON.parse(raw) as Record<string, unknown>;
        if (typeof o.sql !== 'string' || !o.sql.trim()) return null;
        if (typeof o.chart !== 'object' || o.chart === null) return null;
        return { sql: o.sql, chart: o.chart as DiveChart };
    } catch {
        return null;
    }
}

function prompt(fromExpr: string, card: string, question: string): string {
    return `You generate a chart for Duckle (DuckDB). Write ONE read-only SQL SELECT that answers the question, plus a Vega-Lite chart spec.
Rules: the SQL must read FROM ${fromExpr}. The chart is a Vega-Lite spec with NO data and NO width/height; its encoding fields MUST be output columns of the SELECT.
Output ONLY a \`\`\`json fenced block: {"sql": "SELECT ...", "chart": {"mark": "bar", "encoding": {"x": {"field": "...", "type": "nominal"}, "y": {"field": "...", "type": "quantitative"}}}}.

Table columns:
${card}

Question: ${question}`;
}

/** Describe the source, ask Duckie, parse the result. Throws with the raw reply
 *  on a parse failure so the caller can show it. */
export async function generateDive(
    fromExpr: string,
    question: string,
    workspacePath: string | null,
): Promise<GeneratedDive> {
    const probe = await runDive(
        {
            diveSchemaVersion: 1,
            id: 'probe',
            title: 'probe',
            query: { sql: `SELECT * FROM ${fromExpr} LIMIT 5` },
            chart: {},
        },
        workspacePath,
    );
    const history: ChatMessage[] = [
        { role: 'user', content: prompt(fromExpr, schemaCard(probe.columns, probe.rows), question) },
    ];
    let text = '';
    await new Promise<void>((resolve, reject) => {
        void chatSend(
            history,
            (e) => {
                if (e.kind === 'token') text += e.text;
                else if (e.kind === 'done') resolve();
                else if (e.kind === 'error') reject(new Error(e.message));
            },
            workspacePath,
        );
    });
    const gen = parseGenerated(text);
    if (!gen) {
        throw new Error('Duckie did not return a usable chart. Raw reply:\n' + text.slice(0, 400));
    }
    return gen;
}
