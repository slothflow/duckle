/**
 * Derive a node's canvas subtitle from its live configuration, so the
 * card reflects what the component is actually set to do - not a static
 * label baked in when it was dropped. Returns `undefined` when there's
 * nothing meaningful to show yet (the card then renders just its name).
 */
export function deriveNodeSubtitle(
    componentId: string | undefined,
    props: Record<string, unknown> | undefined,
): string | undefined {
    if (!componentId) return undefined;
    const p = props ?? {};

    const str = (v: unknown): string | undefined =>
        typeof v === 'string' && v.trim() ? v.trim() : undefined;
    const basename = (v: unknown): string | undefined => {
        const s = str(v);
        return s ? s.split(/[\\/]/).pop() || s : undefined;
    };
    const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
    const strs = (v: unknown): string[] =>
        arr(v).filter((x): x is string => typeof x === 'string');

    // Sources & sinks point at a file / table / bucket.
    if (componentId.startsWith('src.') || componentId.startsWith('snk.')) {
        return (
            basename(p.path) ??
            basename(p.url) ??
            str(p.tableName) ??
            basename(p.database) ??
            str(p.bucket)
        );
    }

    switch (componentId) {
        case 'xf.filter': {
            // The predicate is a structured object carrying its compiled
            // SQL; older configs may store a raw string.
            const pred = p.predicate;
            if (typeof pred === 'string') return str(pred);
            if (pred && typeof pred === 'object') {
                const o = pred as { sql?: unknown; rawSql?: unknown; mode?: unknown };
                return str(o.sql) ?? (o.mode === 'raw' ? str(o.rawSql) : undefined);
            }
            return str(p.filterSql);
        }
        case 'xf.project': {
            const n = strs(p.columns).length;
            return n ? `keep ${n} col${n === 1 ? '' : 's'}` : undefined;
        }
        case 'xf.drop':
        case 'xf.dropcol': {
            const n = strs(p.columns).length;
            return n ? `drop ${n} col${n === 1 ? '' : 's'}` : undefined;
        }
        case 'xf.agg':
        case 'xf.groupby': {
            const gb = strs(p.groupBy);
            return gb.length ? `by ${gb.join(', ')}` : undefined;
        }
        case 'xf.sort': {
            const n = arr(p.orderBy).length;
            return n ? `${n} sort key${n === 1 ? '' : 's'}` : undefined;
        }
        case 'xf.limit':
            return p.limit != null ? `limit ${p.limit}` : undefined;
        case 'xf.rename': {
            const n = arr(p.renames).length || arr(p.columns).length;
            return n ? `rename ${n}` : undefined;
        }
        case 'xf.cast': {
            const n = arr(p.casts).length || arr(p.columns).length;
            return n ? `cast ${n}` : undefined;
        }
        case 'xf.map': {
            const outs = arr((p.mapper as { outputs?: unknown[] } | undefined)?.outputs);
            return outs.length ? `${outs.length} output col${outs.length === 1 ? '' : 's'}` : undefined;
        }
        case 'xf.join.inner':
        case 'xf.join.left':
        case 'xf.join.right':
        case 'xf.join.full':
        case 'xf.lookup':
        case 'xf.semi':
        case 'xf.anti': {
            const lk = str(p.leftKey);
            const rk = str(p.rightKey);
            return lk && rk ? `${lk} = ${rk}` : lk ? `on ${lk}` : undefined;
        }
        case 'code.sql':
        case 'code.sqltemplate':
            return str(p.sql) ? 'SQL' : undefined;
        case 'code.python':
        case 'code.rust':
        case 'code.javascript':
            return str(p.language) ?? undefined;
        default:
            return undefined;
    }
}
