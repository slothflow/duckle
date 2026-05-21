import { useContext } from 'react';
import { FieldContext } from './FieldContext';
import { AGG_FUNCTIONS, type Aggregation, type AggregationFunction } from './types';

type Props = {
    value: Aggregation[] | undefined;
    onChange: (v: Aggregation[]) => void;
};

export function AggregationsField({ value, onChange }: Props) {
    const { upstreamSchema } = useContext(FieldContext);
    const aggs = value ?? [];

    const add = () => {
        const first = upstreamSchema[0];
        if (!first) {
            onChange([...aggs, { column: '', func: 'count', output: 'count' }]);
            return;
        }
        onChange([
            ...aggs,
            {
                column: first.name,
                func: 'sum',
                output: 'sum_' + first.name,
            },
        ]);
    };

    const update = (i: number, patch: Partial<Aggregation>) => {
        onChange(aggs.map((a, idx) => (idx === i ? { ...a, ...patch } : a)));
    };

    const remove = (i: number) => {
        onChange(aggs.filter((_, idx) => idx !== i));
    };

    return (
        <div className="field-aggregations">
            <div className="field-agg-toolbar">
                <span className="field-agg-count">
                    {aggs.length} aggregation{aggs.length === 1 ? '' : 's'}
                </span>
                <button type="button" className="schema-add" onClick={add}>
                    + Add aggregation
                </button>
            </div>
            {aggs.length === 0 ? (
                <div className="field-agg-empty">
                    No aggregations defined. Click <b>+ Add aggregation</b> to compute SUM, COUNT,
                    AVG, MIN, MAX, etc.
                </div>
            ) : (
                <div className="field-agg-table">
                    <div className="field-agg-row field-agg-header">
                        <div>Column</div>
                        <div>Function</div>
                        <div>Output</div>
                        <div />
                    </div>
                    {aggs.map((a, i) => (
                        <div className="field-agg-row" key={i}>
                            <select
                                className="schema-input"
                                value={a.column}
                                onChange={e => {
                                    const col = e.target.value;
                                    update(i, {
                                        column: col,
                                        output:
                                            a.output && a.output !== a.func + '_' + a.column
                                                ? a.output
                                                : a.func + '_' + col,
                                    });
                                }}
                            >
                                <option value="">— column —</option>
                                {upstreamSchema.map(c => (
                                    <option key={c.name} value={c.name}>
                                        {c.name}
                                    </option>
                                ))}
                            </select>
                            <select
                                className="schema-input"
                                value={a.func}
                                onChange={e =>
                                    update(i, {
                                        func: e.target.value as AggregationFunction,
                                        output:
                                            a.output && a.output !== a.func + '_' + a.column
                                                ? a.output
                                                : (e.target.value as string) + '_' + a.column,
                                    })
                                }
                            >
                                {AGG_FUNCTIONS.map(f => (
                                    <option key={f} value={f}>
                                        {f}
                                    </option>
                                ))}
                            </select>
                            <input
                                type="text"
                                className="schema-input"
                                value={a.output}
                                onChange={e => update(i, { output: e.target.value })}
                                spellCheck={false}
                            />
                            <button
                                type="button"
                                className="schema-remove"
                                onClick={() => remove(i)}
                                aria-label="Remove"
                            >
                                ×
                            </button>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
