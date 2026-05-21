import { useMemo, useState } from 'react';
import type { Edge, Node } from '@xyflow/react';
import type { Column, DuckleNodeData } from '../pipeline-types';
import SchemaEditor from './SchemaEditor';
import FieldRenderer from './fields/FieldRenderer';
import { FieldContext } from './fields/FieldContext';
import { getManifest } from './fields/component-manifests';

type TabId = 'basic' | 'schema' | 'preview' | 'advanced' | 'validation';

const KIND_LABEL: Record<string, string> = {
    source: 'Source',
    transform: 'Transform',
    sink: 'Sink',
};

const KIND_COLOR: Record<string, string> = {
    source: '#7ee787',
    transform: '#58a6ff',
    sink: '#ffa657',
};

type Props = {
    selected: Node<DuckleNodeData> | null;
    allNodes: Node<DuckleNodeData>[];
    edges: Edge[];
    onUpdate: (id: string, patch: Partial<DuckleNodeData>) => void;
};

export default function PropertiesPanel({ selected, allNodes, edges, onUpdate }: Props) {
    const [tab, setTab] = useState<TabId>('basic');
    const [autodetecting, setAutodetecting] = useState(false);

    const upstreamSchema = useMemo<Column[]>(() => {
        if (!selected) return [];
        const upstreamIds = edges.filter(e => e.target === selected.id).map(e => e.source);
        const cols: Column[] = [];
        const seen = new Set<string>();
        for (const id of upstreamIds) {
            const node = allNodes.find(n => n.id === id);
            const schema = node?.data.schema ?? [];
            for (const c of schema) {
                if (!seen.has(c.name)) {
                    seen.add(c.name);
                    cols.push(c);
                }
            }
        }
        return cols;
    }, [selected, edges, allNodes]);

    if (!selected) {
        return (
            <aside className="properties">
                <div className="properties-empty">
                    <svg
                        width="40"
                        height="40"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.6"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        aria-hidden="true"
                    >
                        <circle cx="12" cy="12" r="9" />
                        <line x1="12" y1="8" x2="12" y2="12" />
                        <circle cx="12" cy="16" r="0.5" fill="currentColor" />
                    </svg>
                    <div className="properties-empty-title">Nothing selected</div>
                    <div className="properties-empty-desc">
                        Click a node on the canvas to edit its configuration, schema, and validation
                        rules.
                    </div>
                </div>
            </aside>
        );
    }

    const kind = (selected.type ?? 'transform') as string;
    const data = selected.data;
    const props = data.properties ?? {};
    const manifest = getManifest(data.componentId);
    const declaredSchema = data.schema ?? [];

    const TABS: { id: TabId; label: string }[] = [
        { id: 'basic', label: 'Basic' },
        { id: 'schema', label: 'Schema' },
        { id: 'preview', label: 'Preview' },
        { id: 'advanced', label: 'Advanced' },
        { id: 'validation', label: 'Validation' },
    ];

    const setLabel = (label: string) => onUpdate(selected.id, { label });
    const setProperty = (key: string, value: unknown) =>
        onUpdate(selected.id, { properties: { ...props, [key]: value } });
    const setSchema = (columns: Column[]) => onUpdate(selected.id, { schema: columns });

    const runAutodetect = async () => {
        if (!manifest?.autodetect) return;
        setAutodetecting(true);
        try {
            const result = await manifest.autodetect();
            onUpdate(selected.id, {
                schema: result.columns,
                sampleRows: result.sampleRows,
            });
        } finally {
            setAutodetecting(false);
        }
    };

    return (
        <aside className="properties">
            <div className="properties-header">
                <div className="properties-kind-row">
                    <span
                        className="properties-kind-dot"
                        style={{ background: KIND_COLOR[kind] ?? '#666' }}
                        aria-hidden="true"
                    />
                    <span className="properties-kind">{KIND_LABEL[kind] ?? kind}</span>
                    <span className="properties-id">#{selected.id}</span>
                </div>
                <input
                    type="text"
                    className="properties-name-input"
                    value={data.label}
                    onChange={e => setLabel(e.target.value)}
                    placeholder="Component name"
                    spellCheck={false}
                />
                {manifest ? (
                    <div className="properties-manifest-row">
                        <code className="properties-manifest-id">{manifest.id}</code>
                        {manifest.description ? (
                            <span className="properties-manifest-desc">{manifest.description}</span>
                        ) : null}
                    </div>
                ) : (
                    <div className="properties-manifest-row">
                        <span className="properties-manifest-warn">
                            No manifest registered for <code>{data.componentId ?? 'untyped'}</code>
                        </span>
                    </div>
                )}
            </div>

            <div className="properties-tabs" role="tablist">
                {TABS.map(t => (
                    <button
                        key={t.id}
                        type="button"
                        role="tab"
                        aria-selected={tab === t.id}
                        className="properties-tab"
                        onClick={() => setTab(t.id)}
                    >
                        {t.label}
                    </button>
                ))}
            </div>

            <FieldContext.Provider value={{ upstreamSchema, nodeSchema: declaredSchema }}>
                <div className="properties-content">
                    {tab === 'basic' ? (
                        <div className="properties-section">
                            {manifest ? (
                                manifest.sections.map(section => (
                                    <div className="form-section" key={section.label}>
                                        <div className="form-section-label">{section.label}</div>
                                        {section.fields.map(field => (
                                            <FieldRenderer
                                                key={field.key}
                                                field={field}
                                                value={
                                                    props[field.key] !== undefined
                                                        ? props[field.key]
                                                        : field.defaultValue
                                                }
                                                onChange={v => setProperty(field.key, v)}
                                            />
                                        ))}
                                    </div>
                                ))
                            ) : (
                                <div className="properties-hint">
                                    This component has no registered manifest. Configure
                                    component-specific properties by registering a manifest in{' '}
                                    <code>component-manifests.ts</code>.
                                </div>
                            )}
                        </div>
                    ) : null}

                    {tab === 'schema' ? (
                        <div className="properties-section">
                            {manifest?.schemaSource === 'upstream' ? (
                                <div className="schema-source-banner">
                                    Schema inherited from upstream
                                </div>
                            ) : null}
                            {manifest?.schemaSource === 'autodetect' ? (
                                <div className="schema-autodetect-row">
                                    <button
                                        type="button"
                                        className="schema-autodetect-button"
                                        onClick={runAutodetect}
                                        disabled={autodetecting}
                                    >
                                        {autodetecting ? 'Detecting…' : 'Autodetect from source'}
                                    </button>
                                    <span className="schema-autodetect-hint">
                                        Reads the file header and a sample of rows to infer types.
                                    </span>
                                </div>
                            ) : null}
                            {manifest?.schemaSource === 'declared' ? (
                                <div className="schema-source-banner schema-source-banner-declared">
                                    Declared schema — define the output columns explicitly.
                                </div>
                            ) : null}
                            <SchemaEditor
                                columns={
                                    manifest?.schemaSource === 'upstream'
                                        ? upstreamSchema
                                        : declaredSchema
                                }
                                onChange={setSchema}
                                readOnly={manifest?.schemaSource === 'upstream'}
                            />
                        </div>
                    ) : null}

                    {tab === 'preview' ? (
                        <div className="properties-section">
                            <PreviewTab
                                schema={
                                    manifest?.schemaSource === 'upstream'
                                        ? upstreamSchema
                                        : declaredSchema
                                }
                                rows={data.sampleRows ?? []}
                            />
                        </div>
                    ) : null}

                    {tab === 'advanced' ? (
                        <div className="properties-section">
                            <div className="properties-placeholder">
                                <div className="properties-placeholder-title">Advanced settings</div>
                                <div className="properties-placeholder-desc">
                                    Buffering, parallelism, retry policy, custom partitioning,
                                    encoding options, and other rarely-touched knobs will live here.
                                </div>
                            </div>
                        </div>
                    ) : null}

                    {tab === 'validation' ? (
                        <div className="properties-section">
                            <div className="validation-summary validation-ok">
                                <span className="validation-icon" aria-hidden="true">
                                    ✓
                                </span>
                                <span>No issues detected for this node.</span>
                            </div>
                            <div className="properties-hint">
                                Schema mismatches, missing required properties, and engine
                                compatibility warnings will surface here.
                            </div>
                        </div>
                    ) : null}
                </div>
            </FieldContext.Provider>
        </aside>
    );
}

type PreviewProps = {
    schema: Column[];
    rows: Record<string, unknown>[];
};

function PreviewTab({ schema, rows }: PreviewProps) {
    if (schema.length === 0) {
        return (
            <div className="preview-empty">
                <div className="preview-empty-title">No schema yet</div>
                <div className="preview-empty-desc">
                    Run schema autodetect (Schema tab) or connect an upstream node to see sample
                    data here.
                </div>
            </div>
        );
    }

    if (rows.length === 0) {
        return (
            <div className="preview-empty">
                <div className="preview-empty-title">No sample rows</div>
                <div className="preview-empty-desc">
                    Click <b>Autodetect</b> on the Schema tab to pull a sample, or run the pipeline
                    to fill this in.
                </div>
            </div>
        );
    }

    const cols = schema.map(c => c.name);
    return (
        <div className="preview-wrap">
            <div className="preview-meta">
                {rows.length} sample row{rows.length === 1 ? '' : 's'} · {cols.length} column
                {cols.length === 1 ? '' : 's'}
            </div>
            <div className="preview-tablewrap">
                <table className="preview-table">
                    <thead>
                        <tr>
                            {schema.map(c => (
                                <th key={c.name}>
                                    <div className="preview-th-name">{c.name}</div>
                                    <div className="preview-th-type">{c.type}</div>
                                </th>
                            ))}
                        </tr>
                    </thead>
                    <tbody>
                        {rows.map((r, i) => (
                            <tr key={i}>
                                {cols.map(name => (
                                    <td key={name}>{formatCell(r[name])}</td>
                                ))}
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    );
}

function formatCell(v: unknown): string {
    if (v === null || v === undefined) return '∅';
    if (typeof v === 'object') return JSON.stringify(v);
    return String(v);
}
