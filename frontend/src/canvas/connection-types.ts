export type ConnectionType =
    | 'main'
    | 'lookup'
    | 'reject'
    | 'filter'
    | 'iterate'
    | 'on-subjob-ok'
    | 'on-subjob-error'
    | 'on-component-ok'
    | 'on-component-error'
    | 'if'
    | 'run-if';

export type ConnectionGroup = 'row' | 'trigger';

export type ConnectionTypeMeta = {
    id: ConnectionType;
    label: string;
    group: ConnectionGroup;
    description: string;
    color: string;
    dash: string | null;
    width: number;
    badge: string | null;
    expressionRequired?: boolean;
};

export const CONNECTION_TYPES: ConnectionTypeMeta[] = [
    {
        id: 'main',
        label: 'Main',
        group: 'row',
        description: 'Default flow. Rows pass through unchanged.',
        color: 'var(--text-2)',
        dash: null,
        width: 1.6,
        badge: null,
    },
    {
        id: 'lookup',
        label: 'Lookup',
        group: 'row',
        description: 'Reference data for joins / mapper lookups. Loaded then keyed.',
        color: 'var(--kind-source)',
        dash: '6 4',
        width: 1.6,
        badge: 'lookup',
    },
    {
        id: 'reject',
        label: 'Reject',
        group: 'row',
        description: 'Rows that failed validation or matching.',
        color: 'var(--danger)',
        dash: '5 4',
        width: 1.6,
        badge: 'reject',
    },
    {
        id: 'filter',
        label: 'Filter',
        group: 'row',
        description: 'Rows filtered out by the upstream component.',
        color: 'var(--kind-sink)',
        dash: '6 4',
        width: 1.6,
        badge: 'filter',
    },
    {
        id: 'iterate',
        label: 'Iterate',
        group: 'row',
        description: 'Iterate over inputs; one downstream run per row.',
        color: 'var(--kind-control)',
        dash: '3 3',
        width: 1.6,
        badge: 'iterate',
    },
    {
        id: 'on-subjob-ok',
        label: 'On Subjob OK',
        group: 'trigger',
        description: 'Run the next subjob once this one completes successfully.',
        color: 'var(--success)',
        dash: '2 3',
        width: 1,
        badge: 'SUBJOB OK',
    },
    {
        id: 'on-subjob-error',
        label: 'On Subjob Error',
        group: 'trigger',
        description: 'Run the next subjob if this one fails.',
        color: 'var(--danger)',
        dash: '2 3',
        width: 1,
        badge: 'SUBJOB ERROR',
    },
    {
        id: 'on-component-ok',
        label: 'On Component OK',
        group: 'trigger',
        description: 'Fire when this component finishes without error.',
        color: 'var(--success)',
        dash: '4 3',
        width: 1,
        badge: 'OK',
    },
    {
        id: 'on-component-error',
        label: 'On Component Error',
        group: 'trigger',
        description: 'Fire when this component errors.',
        color: 'var(--danger)',
        dash: '4 3',
        width: 1,
        badge: 'ERROR',
    },
    {
        id: 'if',
        label: 'If',
        group: 'trigger',
        description: 'Conditional branch - provide a boolean expression.',
        color: 'var(--accent)',
        dash: '5 3',
        width: 1,
        badge: 'if',
        expressionRequired: true,
    },
    {
        id: 'run-if',
        label: 'Run If',
        group: 'trigger',
        description: 'Run-time conditional with a richer expression.',
        color: 'var(--kind-quality)',
        dash: '5 3',
        width: 1,
        badge: 'run if',
        expressionRequired: true,
    },
];

export function metaFor(type: ConnectionType): ConnectionTypeMeta {
    return CONNECTION_TYPES.find(t => t.id === type) ?? CONNECTION_TYPES[0]!;
}

export const ROW_CONNECTIONS = CONNECTION_TYPES.filter(t => t.group === 'row');
export const TRIGGER_CONNECTIONS = CONNECTION_TYPES.filter(t => t.group === 'trigger');
