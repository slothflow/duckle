import type { Field } from './types';

type Props<T> = {
    field: Field;
    value: T | undefined;
    onChange: (v: T) => void;
};

export function TextField({ field, value, onChange }: Props<string>) {
    return (
        <input
            type="text"
            className="field-input"
            value={value ?? ''}
            placeholder={field.placeholder}
            onChange={e => onChange(e.target.value)}
            spellCheck={false}
        />
    );
}

export function TextareaField({ field, value, onChange }: Props<string>) {
    return (
        <textarea
            className={'field-input field-textarea' + (field.monospace ? ' field-mono' : '')}
            value={value ?? ''}
            placeholder={field.placeholder}
            rows={field.rows ?? 3}
            onChange={e => onChange(e.target.value)}
            spellCheck={false}
        />
    );
}

export function NumberField({ field, value, onChange }: Props<number>) {
    return (
        <input
            type="number"
            className="field-input"
            value={value ?? ''}
            placeholder={field.placeholder}
            onChange={e => {
                const n = e.target.value === '' ? NaN : Number(e.target.value);
                onChange(Number.isFinite(n) ? n : 0);
            }}
        />
    );
}

export function IntegerField({ field, value, onChange }: Props<number>) {
    return (
        <input
            type="number"
            step={1}
            className="field-input"
            value={value ?? ''}
            placeholder={field.placeholder}
            onChange={e => {
                const n = e.target.value === '' ? NaN : parseInt(e.target.value, 10);
                onChange(Number.isFinite(n) ? n : 0);
            }}
        />
    );
}

export function BoolField({ field, value, onChange }: Props<boolean>) {
    return (
        <label className="field-toggle">
            <input
                type="checkbox"
                checked={value ?? false}
                onChange={e => onChange(e.target.checked)}
            />
            <span className="field-toggle-label">{field.placeholder ?? 'Enabled'}</span>
        </label>
    );
}

export function SelectField({ field, value, onChange }: Props<string>) {
    return (
        <select
            className="field-input field-select"
            value={value ?? ''}
            onChange={e => onChange(e.target.value)}
        >
            {field.options?.map(o => (
                <option key={o.value} value={o.value}>
                    {o.label}
                </option>
            ))}
        </select>
    );
}
