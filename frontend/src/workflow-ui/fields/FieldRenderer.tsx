import type { Field, Aggregation } from './types';
import {
    BoolField,
    IntegerField,
    NumberField,
    SelectField,
    TextField,
    TextareaField,
} from './PrimitiveFields';
import { FilePathField } from './FilePathField';
import { ExpressionField } from './ExpressionField';
import { ColumnField, ColumnsField } from './ColumnField';
import { AggregationsField } from './AggregationsField';
import { KeyValueField } from './KeyValueField';

type Props = {
    field: Field;
    value: unknown;
    onChange: (v: unknown) => void;
};

export default function FieldRenderer({ field, value, onChange }: Props) {
    return (
        <div className="form-field">
            <label className="form-field-label">
                {field.label}
                {field.required ? <span className="form-field-required">*</span> : null}
            </label>
            {renderInput(field, value, onChange)}
            {field.description ? (
                <div className="form-field-desc">{field.description}</div>
            ) : null}
        </div>
    );
}

function renderInput(field: Field, value: unknown, onChange: (v: unknown) => void): React.ReactNode {
    switch (field.kind) {
        case 'text':
            return <TextField field={field} value={value as string | undefined} onChange={onChange} />;
        case 'textarea':
            return (
                <TextareaField field={field} value={value as string | undefined} onChange={onChange} />
            );
        case 'number':
            return <NumberField field={field} value={value as number | undefined} onChange={onChange} />;
        case 'integer':
            return (
                <IntegerField field={field} value={value as number | undefined} onChange={onChange} />
            );
        case 'bool':
            return <BoolField field={field} value={value as boolean | undefined} onChange={onChange} />;
        case 'select':
            return <SelectField field={field} value={value as string | undefined} onChange={onChange} />;
        case 'file-path':
            return (
                <FilePathField
                    field={field}
                    value={value as string | undefined}
                    onChange={onChange}
                    mode="open"
                />
            );
        case 'save-path':
            return (
                <FilePathField
                    field={field}
                    value={value as string | undefined}
                    onChange={onChange}
                    mode="save"
                />
            );
        case 'expression':
            return (
                <ExpressionField
                    field={field}
                    value={value as string | undefined}
                    onChange={onChange}
                />
            );
        case 'column':
            return (
                <ColumnField field={field} value={value as string | undefined} onChange={onChange} />
            );
        case 'columns':
            return (
                <ColumnsField field={field} value={value as string[] | undefined} onChange={onChange} />
            );
        case 'aggregations':
            return (
                <AggregationsField value={value as Aggregation[] | undefined} onChange={onChange} />
            );
        case 'key-value':
            return (
                <KeyValueField
                    value={value as { key: string; value: string }[] | undefined}
                    onChange={onChange}
                />
            );
    }
}
