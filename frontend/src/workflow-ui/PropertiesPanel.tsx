import { useEffect, useMemo, useRef, useState } from 'react';
import type { Edge, Node } from '@xyflow/react';
import { CheckCircle2, MousePointer2 } from 'lucide-react';
import { resolveUpstreamSchema, resolveUpstreamSampleRows } from '../schema-resolve';
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
    focusNameRequest?: number;
};

export default function PropertiesPanel({
    selected,
    allNodes,
    edges,
    onUpdate,
    focusNameRequest,
}: Props) {
    const [tab, setTab] = useState<TabId>('basic');
    const [autodetecting, setAutodetecting] = useState(false);
    const nameInputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (focusNameRequest && nameInputRef.current) {
            setTab('basic');
            const el = nameInputRef.current;
            setTimeout(() => {
                el.focus();
                el.select();
            }, 50);
        }
    }, [focusNameRequest]);

    const upstreamSchema = useMemo<Column[]>(
        () => resolveUpstreamSchema(selected?.id, allNodes, edges),
        [selected, edges, allNodes],
    );

    const upstreamSampleRows = useMemo<Record<string, unknown>[]>(
        () => resolveUpstreamSampleRows(selected?.id, allNodes, edges),
        [selected, edges, allNodes],
    );

    if (!selected) {
        return (
            <aside className="properties">
                <div className="properties-empty">
                    <MousePointer2 size={32} strokeWidth={1.4} />
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
                    ref={nameInputRef}
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
                ) : null}
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
                                    Generic component. Add a note in the field above to describe
                                    what this node should do.
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
                                        : declaredSchema.length > 0
                                          ? declaredSchema
                                          : upstreamSchema
                                }
                                rows={
                                    data.sampleRows && data.sampleRows.length > 0
                                        ? data.sampleRows
                                        : upstreamSampleRows
                                }
                                inheritedRows={
                                    (!data.sampleRows || data.sampleRows.length === 0) &&
                                    upstreamSampleRows.length > 0
                                }
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
                                <CheckCircle2 size={14} className="validation-icon" aria-hidden="true" />
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
    inheritedRows?: boolean;
};

function PreviewTab({ schema, rows, inheritedRows }: PreviewProps) {
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
                {inheritedRows ? (
                    <span className="preview-meta-tag"> · upstream sample</span>
                ) : null}
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
