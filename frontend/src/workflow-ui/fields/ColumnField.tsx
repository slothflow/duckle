import { useContext } from 'react';
import type { Field } from './types';
import { FieldContext } from './FieldContext';

type Props = {
    field: Field;
    value: string | undefined;
    onChange: (v: string) => void;
};

export function ColumnField({ field, value, onChange }: Props) {
    const { upstreamSchema } = useContext(FieldContext);

    if (upstreamSchema.length === 0) {
        return (
            <div className="field-input field-warning">
                No upstream schema. Connect an input to populate this list.
            </div>
        );
    }

    return (
        <select
            className="field-input field-select"
            value={value ?? ''}
            onChange={e => onChange(e.target.value)}
        >
            <option value="">— select column —</option>
            {upstreamSchema.map(c => (
                <option key={c.name} value={c.name}>
                    {c.name}  ({c.type})
                </option>
            ))}
        </select>
    );
}

type MultiProps = {
    field: Field;
    value: string[] | undefined;
    onChange: (v: string[]) => void;
};

export function ColumnsField({ value, onChange }: MultiProps) {
    const { upstreamSchema } = useContext(FieldContext);
    const selected = new Set(value ?? []);

    if (upstreamSchema.length === 0) {
        return (
            <div className="field-input field-warning">
                No upstream schema. Connect an input to populate this list.
            </div>
        );
    }

    const toggle = (name: string) => {
        const next = new Set(selected);
        if (next.has(name)) next.delete(name);
        else next.add(name);
        onChange(Array.from(next));
    };

    return (
        <div className="field-columns">
            {upstreamSchema.map(c => (
                <label key={c.name} className="field-columns-row">
                    <input
                        type="checkbox"
                        checked={selected.has(c.name)}
                        onChange={() => toggle(c.name)}
                    />
                    <span className="field-columns-name">{c.name}</span>
                    <span className="field-columns-type">{c.type}</span>
                </label>
            ))}
        </div>
    );
}
