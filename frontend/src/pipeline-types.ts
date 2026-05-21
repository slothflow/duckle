export type DataType =
    | 'string'
    | 'int32'
    | 'int64'
    | 'float32'
    | 'float64'
    | 'bool'
    | 'date'
    | 'timestamp'
    | 'time'
    | 'decimal'
    | 'json'
    | 'binary';

export const DATA_TYPES: DataType[] = [
    'string',
    'int32',
    'int64',
    'float32',
    'float64',
    'bool',
    'date',
    'timestamp',
    'time',
    'decimal',
    'json',
    'binary',
];

export type Column = {
    name: string;
    type: DataType;
    nullable: boolean;
    primaryKey?: boolean;
};

export type NodeKind = 'source' | 'transform' | 'sink';

export type DuckleNodeData = {
    label: string;
    subtitle?: string;
    componentId?: string;
    properties?: Record<string, unknown>;
    schema?: Column[];
    sampleRows?: Record<string, unknown>[];
    [key: string]: unknown;
};
