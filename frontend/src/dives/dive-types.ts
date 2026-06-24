// Live data views ("dives"): a saved, hand-editable artifact that re-runs its
// SQL against DuckDB on every open (never a cached result) and renders an
// interactive chart. See docs/design/dives.md. Clean-room format on open libs.

export const DIVE_SCHEMA_VERSION = 1;

/**
 * Where a dive reads from. Always a PERSISTENT store (a DuckDB/parquet/csv file
 * a pipeline wrote, or a loose data file) - never an ephemeral pipeline view,
 * which the run's temp DB deletes after each run.
 */
export type DiveSource =
    | { kind: 'duckdb'; database: string; table: string }
    | { kind: 'parquet'; path: string }
    | { kind: 'csv'; path: string };

export type DiveParamType = 'string' | 'number' | 'date' | 'bool';

export interface DiveParam {
    name: string;
    label?: string;
    type: DiveParamType;
    default?: string | number | boolean;
    /** Optional UI control hint; values are restricted to `options` when set. */
    control?: 'text' | 'number' | 'select' | 'date' | 'toggle';
    options?: (string | number)[];
}

export interface DiveQuery {
    /**
     * A single read-only SELECT. Params are resolved to safe typed literals
     * before this reaches the engine - never free-text concatenation.
     */
    sql: string;
    params?: DiveParam[];
}

/**
 * A Vega-Lite top-level spec WITHOUT inline data; rows are injected at runtime
 * into a named dataset, which keeps the file tiny and makes "never-stale"
 * automatic. Left structurally open here so the format module carries no hard
 * vega-lite type dependency; the renderer validates the spec.
 */
export type DiveChart = Record<string, unknown>;

export interface DiveState {
    paramValues?: Record<string, string | number | boolean>;
    sort?: unknown;
    filters?: unknown;
    drill?: unknown;
    rowLimit?: number;
}

export interface DiveMeta {
    createdAt?: string;
    updatedAt?: string;
    generator?: 'duckie' | 'manual';
    prompt?: string;
    rev?: number;
}

export interface Dive {
    diveSchemaVersion: number;
    id: string;
    title: string;
    description?: string;
    question?: string;
    // Optional in v1: a self-contained SQL dive reads its source inline (e.g.
    // read_parquet('...')). Set for the duckdb-attach case (a later phase).
    source?: DiveSource;
    query: DiveQuery;
    chart: DiveChart;
    state?: DiveState;
    meta?: DiveMeta;
}

export interface DiveParseResult {
    ok: boolean;
    dive?: Dive;
    error?: string;
}

/**
 * Validate and narrow an unknown (parsed JSON) into a Dive. Rejects an unknown
 * MAJOR schema version and anything missing the load-bearing fields, so a bad
 * or future-versioned dive file fails loudly instead of half-rendering.
 */
export function parseDive(raw: unknown): DiveParseResult {
    if (typeof raw !== 'object' || raw === null) {
        return { ok: false, error: 'Dive is not a JSON object.' };
    }
    const d = raw as Record<string, unknown>;
    const ver = d.diveSchemaVersion;
    if (typeof ver !== 'number' || Math.floor(ver) > DIVE_SCHEMA_VERSION) {
        return {
            ok: false,
            error: `Unsupported dive schema version: ${String(ver)} (this build reads up to ${DIVE_SCHEMA_VERSION}).`,
        };
    }
    if (typeof d.id !== 'string' || !d.id) return { ok: false, error: 'Dive is missing "id".' };
    if (typeof d.title !== 'string' || !d.title) return { ok: false, error: 'Dive is missing "title".' };
    const q = d.query as Record<string, unknown> | undefined;
    if (!q || typeof q !== 'object' || typeof q.sql !== 'string' || !q.sql.trim()) {
        return { ok: false, error: 'Dive "query.sql" is required.' };
    }
    if (typeof d.chart !== 'object' || d.chart === null) {
        return { ok: false, error: 'Dive "chart" spec is required.' };
    }
    return { ok: true, dive: raw as Dive };
}
