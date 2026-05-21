import type { FileFilter } from '../../tauri-dialog';
import type { Column, NodeKind } from '../../pipeline-types';

export type FieldKind =
    | 'text'
    | 'textarea'
    | 'number'
    | 'integer'
    | 'bool'
    | 'select'
    | 'file-path'
    | 'save-path'
    | 'expression'
    | 'column'
    | 'columns'
    | 'aggregations'
    | 'key-value';

export type SelectOption = { label: string; value: string };

export type Field = {
    key: string;
    label: string;
    kind: FieldKind;
    description?: string;
    required?: boolean;
    defaultValue?: unknown;
    placeholder?: string;
    options?: SelectOption[];
    filters?: FileFilter[];
    monospace?: boolean;
    rows?: number;
};

export type FormSection = {
    label: string;
    fields: Field[];
    collapsible?: boolean;
    defaultCollapsed?: boolean;
};

export type SchemaSource = 'upstream' | 'declared' | 'autodetect';

export type AutodetectResult = {
    columns: Column[];
    sampleRows?: Record<string, unknown>[];
};

export type ComponentManifest = {
    id: string;
    kind: NodeKind;
    label: string;
    description?: string;
    sections: FormSection[];
    schemaSource: SchemaSource;
    autodetect?: () => Promise<AutodetectResult>;
};

export type AggregationFunction =
    | 'count'
    | 'sum'
    | 'avg'
    | 'min'
    | 'max'
    | 'first'
    | 'last'
    | 'count_distinct'
    | 'array_agg';

export const AGG_FUNCTIONS: AggregationFunction[] = [
    'count',
    'sum',
    'avg',
    'min',
    'max',
    'first',
    'last',
    'count_distinct',
    'array_agg',
];

export type Aggregation = {
    column: string;
    func: AggregationFunction;
    output: string;
};
