import type { Field } from './types';

type Props = {
    field: Field;
    value: string | undefined;
    onChange: (v: string) => void;
};

export function ExpressionField({ field, value, onChange }: Props) {
    return (
        <div className="field-expression">
            <div className="field-expression-bar">
                <span className="field-expression-lang">SQL</span>
                <span className="field-expression-hint">
                    e.g. <code>status = 'paid' AND amount &gt; 100</code>
                </span>
            </div>
            <textarea
                className="field-input field-textarea field-mono field-expression-area"
                value={value ?? ''}
                placeholder={field.placeholder ?? "column = 'value'"}
                rows={field.rows ?? 4}
                onChange={e => onChange(e.target.value)}
                spellCheck={false}
            />
        </div>
    );
}
