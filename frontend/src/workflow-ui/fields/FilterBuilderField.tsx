import { useContext, useMemo } from 'react';
import { Code, LayoutList } from 'lucide-react';
import { FieldContext } from './FieldContext';
import type { Column } from '../../pipeline-types';

export type FilterOp =
    | 'eq'
    | 'neq'
    | 'gt'
    | 'lt'
    | 'gte'
    | 'lte'
    | 'between'
    | 'in'
    | 'not_in'
    | 'like'
    | 'not_like'
    | 'is_null'
    | 'is_not_null'
    | 'starts_with'
    | 'ends_with'
    | 'contains';

type OpMeta = {
    value: FilterOp;
    label: string;
    args: 0 | 1 | 2 | 'csv';
    placeholder?: string;
};

const OP_OPTIONS: OpMeta[] = [
    { value: 'eq', label: '=', args: 1 },
    { value: 'neq', label: '!=', args: 1 },
    { value: 'gt', label: '>', args: 1 },
    { value: 'lt', label: '<', args: 1 },
    { value: 'gte', label: '>=', args: 1 },
    { value: 'lte', label: '<=', args: 1 },
    { value: 'between', label: 'between', args: 2 },
    { value: 'in', label: 'in', args: 'csv', placeholder: 'a, b, c' },
    { value: 'not_in', label: 'not in', args: 'csv', placeholder: 'a, b, c' },
    { value: 'like', label: 'matches', args: 1, placeholder: 'pattern%' },
    { value: 'not_like', label: 'does not match', args: 1, placeholder: 'pattern%' },
    { value: 'starts_with', label: 'starts with', args: 1 },
    { value: 'ends_with', label: 'ends with', args: 1 },
    { value: 'contains', label: 'contains', args: 1 },
    { value: 'is_null', label: 'is null', args: 0 },
    { value: 'is_not_null', label: 'is not null', args: 0 },
];

export type Condition = {
    id: string;
    column: string;
    op: FilterOp;
    value?: string;
    value2?: string;
};

export type FilterPredicate = {
    mode: 'builder' | 'raw';
    match: 'all' | 'any';
    conditions: Condition[];
    rawSql?: string;
    /** Compiled SQL - kept in sync on every edit so the canvas card
     *  and the Rust executor read one authoritative string. */
    sql?: string;
};

function newId(): string {
    return 'c_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 7);
}

function defaultPredicate(): FilterPredicate {
    return { mode: 'builder', match: 'all', conditions: [] };
}

const NUMERIC_TYPES = new Set(['int32', 'int64', 'float32', 'float64', 'decimal']);

function isNumeric(type: string): boolean {
    return NUMERIC_TYPES.has(type);
}

function quoteValue(v: string, type: string): string {
    if (isNumeric(type)) return v || '0';
    return "'" + v.replace(/'/g, "''") + "'";
}

function csvList(v: string, type: string): string {
    return v
        .split(',')
        .map(p => p.trim())
        .filter(p => p.length > 0)
        .map(p => quoteValue(p, type))
        .join(', ');
}

export function conditionToSql(c: Condition, columnType: string): string {
    const col = c.column || '<column>';
    const v = c.value ?? '';
    const v2 = c.value2 ?? '';
    switch (c.op) {
        case 'eq':
            return col + ' = ' + quoteValue(v, columnType);
        case 'neq':
            return col + ' != ' + quoteValue(v, columnType);
        case 'gt':
            return col + ' > ' + quoteValue(v, columnType);
        case 'lt':
            return col + ' < ' + quoteValue(v, columnType);
        case 'gte':
            return col + ' >= ' + quoteValue(v, columnType);
        case 'lte':
            return col + ' <= ' + quoteValue(v, columnType);
        case 'between':
            return (
                col +
                ' BETWEEN ' +
                quoteValue(v, columnType) +
                ' AND ' +
                quoteValue(v2, columnType)
            );
        case 'in':
            return col + ' IN (' + csvList(v, columnType) + ')';
        case 'not_in':
            return col + ' NOT IN (' + csvList(v, columnType) + ')';
        case 'like':
            return col + ' LIKE ' + quoteValue(v, 'string');
        case 'not_like':
            return col + ' NOT LIKE ' + quoteValue(v, 'string');
        case 'starts_with':
            return col + ' LIKE ' + quoteValue(v + '%', 'string');
        case 'ends_with':
            return col + ' LIKE ' + quoteValue('%' + v, 'string');
        case 'contains':
            return col + ' LIKE ' + quoteValue('%' + v + '%', 'string');
        case 'is_null':
            return col + ' IS NULL';
        case 'is_not_null':
            return col + ' IS NOT NULL';
    }
}

export function predicateToSql(p: FilterPredicate, schema: Column[]): string {
    if (p.mode === 'raw') return (p.rawSql ?? '').trim();
    if (p.conditions.length === 0) return '';
    const typeFor = (col: string) => schema.find(c => c.name === col)?.type ?? 'string';
    const parts = p.conditions.map(c => conditionToSql(c, typeFor(c.column)));
    return parts.join(p.match === 'all' ? ' AND ' : ' OR ');
}

type Props = {
    value: unknown;
    onChange: (v: FilterPredicate) => void;
};

export function FilterBuilderField({ value, onChange }: Props) {
    const { upstreamSchema } = useContext(FieldContext);

    const predicate = useMemo<FilterPredicate>(() => {
        if (typeof value === 'string') {
            return { mode: 'raw', match: 'all', conditions: [], rawSql: value };
        }
        if (value && typeof value === 'object' && 'mode' in (value as object)) {
            return value as FilterPredicate;
        }
        return defaultPredicate();
    }, [value]);

    const generatedSql = useMemo(
        () => predicateToSql(predicate, upstreamSchema),
        [predicate, upstreamSchema],
    );

    // Always persist the compiled SQL alongside the structured predicate
    // so the canvas card and the executor read one authoritative value.
    const emit = (next: FilterPredicate) => {
        onChange({ ...next, sql: predicateToSql(next, upstreamSchema) });
    };

    const addCondition = () => {
        const firstCol = upstreamSchema[0]?.name ?? '';
        const newCond: Condition = { id: newId(), column: firstCol, op: 'eq', value: '' };
        emit({ ...predicate, conditions: [...predicate.conditions, newCond] });
    };

    const updateCondition = (id: string, patch: Partial<Condition>) => {
        emit({
            ...predicate,
            conditions: predicate.conditions.map(c => (c.id === id ? { ...c, ...patch } : c)),
        });
    };

    const removeCondition = (id: string) => {
        emit({ ...predicate, conditions: predicate.conditions.filter(c => c.id !== id) });
    };

    const setMode = (mode: 'builder' | 'raw') => {
        if (mode === 'raw') {
            emit({ ...predicate, mode, rawSql: predicate.rawSql ?? generatedSql });
        } else {
            emit({ ...predicate, mode });
        }
    };

    return (
        <div className="filter-builder">
            <div className="filter-builder-modes">
                <button
                    type="button"
                    className={
                        'filter-mode' + (predicate.mode === 'builder' ? ' is-active' : '')
                    }
                    onClick={() => setMode('builder')}
                >
                    <LayoutList size={12} aria-hidden="true" /> Visual
                </button>
                <button
                    type="button"
                    className={'filter-mode' + (predicate.mode === 'raw' ? ' is-active' : '')}
                    onClick={() => setMode('raw')}
                >
                    <Code size={12} aria-hidden="true" /> Raw SQL
                </button>
            </div>

            {predicate.mode === 'builder' ? (
                <>
                    {predicate.conditions.length > 1 ? (
                        <div className="filter-match-row">
                            <span className="filter-match-label">Match:</span>
                            <label className="filter-radio">
                                <input
                                    type="radio"
                                    name="filter-match"
                                    checked={predicate.match === 'all'}
                                    onChange={() => emit({ ...predicate, match: 'all' })}
                                />
                                <span>
                                    <b>All</b> (AND)
                                </span>
                            </label>
                            <label className="filter-radio">
                                <input
                                    type="radio"
                                    name="filter-match"
                                    checked={predicate.match === 'any'}
                                    onChange={() => emit({ ...predicate, match: 'any' })}
                                />
                                <span>
                                    <b>Any</b> (OR)
                                </span>
                            </label>
                        </div>
                    ) : null}

                    {upstreamSchema.length === 0 ? (
                        <div className="field-warning field-input">
                            No upstream schema. Connect an input to add filter conditions.
                        </div>
                    ) : null}

                    {predicate.conditions.length === 0 && upstreamSchema.length > 0 ? (
                        <div className="filter-empty">
                            No conditions yet. Click <b>+ Add condition</b> to filter rows by
                            column.
                        </div>
                    ) : null}

                    {predicate.conditions.map((c, i) => {
                        const opMeta = OP_OPTIONS.find(o => o.value === c.op) ?? OP_OPTIONS[0]!;
                        return (
                            <div className="filter-condition" key={c.id}>
                                <div className="filter-condition-prefix">
                                    {i === 0 ? 'WHERE' : predicate.match === 'all' ? 'AND' : 'OR'}
                                </div>
                                <select
                                    className="schema-input filter-condition-column"
                                    value={c.column}
                                    onChange={e =>
                                        updateCondition(c.id, { column: e.target.value })
                                    }
                                >
                                    <option value="">- column -</option>
                                    {upstreamSchema.map(col => (
                                        <option key={col.name} value={col.name}>
                                            {col.name}
                                        </option>
                                    ))}
                                </select>
                                <select
                                    className="schema-input filter-condition-op"
                                    value={c.op}
                                    onChange={e =>
                                        updateCondition(c.id, { op: e.target.value as FilterOp })
                                    }
                                >
                                    {OP_OPTIONS.map(o => (
                                        <option key={o.value} value={o.value}>
                                            {o.label}
                                        </option>
                                    ))}
                                </select>
                                {opMeta.args === 1 ? (
                                    <input
                                        type="text"
                                        className="schema-input filter-condition-value"
                                        value={c.value ?? ''}
                                        placeholder={opMeta.placeholder ?? 'value'}
                                        onChange={e =>
                                            updateCondition(c.id, { value: e.target.value })
                                        }
                                        spellCheck={false}
                                    />
                                ) : null}
                                {opMeta.args === 2 ? (
                                    <div className="filter-condition-between">
                                        <input
                                            type="text"
                                            className="schema-input"
                                            value={c.value ?? ''}
                                            placeholder="from"
                                            onChange={e =>
                                                updateCondition(c.id, { value: e.target.value })
                                            }
                                            spellCheck={false}
                                        />
                                        <span className="filter-between-sep">and</span>
                                        <input
                                            type="text"
                                            className="schema-input"
                                            value={c.value2 ?? ''}
                                            placeholder="to"
                                            onChange={e =>
                                                updateCondition(c.id, { value2: e.target.value })
                                            }
                                            spellCheck={false}
                                        />
                                    </div>
                                ) : null}
                                {opMeta.args === 'csv' ? (
                                    <input
                                        type="text"
                                        className="schema-input filter-condition-value"
                                        value={c.value ?? ''}
                                        placeholder={opMeta.placeholder ?? 'a, b, c'}
                                        onChange={e =>
                                            updateCondition(c.id, { value: e.target.value })
                                        }
                                        spellCheck={false}
                                    />
                                ) : null}
                                {opMeta.args === 0 ? (
                                    <div className="filter-condition-noop">-</div>
                                ) : null}
                                <button
                                    type="button"
                                    className="schema-remove"
                                    onClick={() => removeCondition(c.id)}
                                    aria-label="Remove condition"
                                >
                                    ×
                                </button>
                            </div>
                        );
                    })}

                    {upstreamSchema.length > 0 ? (
                        <button
                            type="button"
                            className="filter-add-condition"
                            onClick={addCondition}
                        >
                            + Add condition
                        </button>
                    ) : null}

                    {predicate.conditions.length > 0 ? (
                        <div className="filter-sql-preview">
                            <div className="filter-sql-preview-label">Generated SQL</div>
                            <div className="filter-sql-preview-code">
                                {generatedSql || <em>(empty)</em>}
                            </div>
                        </div>
                    ) : null}
                </>
            ) : (
                <div className="filter-raw">
                    <textarea
                        className="field-input field-textarea field-mono"
                        value={predicate.rawSql ?? ''}
                        placeholder="status = 'paid' AND amount > 100"
                        rows={4}
                        onChange={e =>
                            emit({ ...predicate, mode: 'raw', rawSql: e.target.value })
                        }
                        spellCheck={false}
                    />
                    <div className="filter-raw-hint">
                        Raw SQL boolean expression. Switch back to <b>Visual</b> anytime - your
                        conditions are preserved.
                    </div>
                </div>
            )}
        </div>
    );
}
