import { useMemo } from 'react';
import {
    Handle,
    Position,
    useNodes,
    useEdges,
    type Node,
    type NodeProps,
} from '@xyflow/react';
import { AlertCircle, CheckCircle2, Loader2, XCircle } from 'lucide-react';
import type { DuckleNodeData } from '../../pipeline-types';
import { getManifest } from '../../workflow-ui/fields/component-manifests';
import { metaFor } from '../connection-types';
import type { PortDef } from '../../workflow-ui/fields/types';
import { resolveOutputSchema } from '../../schema-resolve';
import { useRunStatus } from '../run-status-context';
import { deriveNodeSubtitle } from '../../node-subtitle';

export type DuckleFlowNode = Node<DuckleNodeData>;

export default function DuckleNode({ id, data, selected, type }: NodeProps<DuckleFlowNode>) {
    const kind = type ?? 'transform';
    const manifest = getManifest(data.componentId);
    const ports = manifest?.ports;
    const inputs = ports?.inputs ?? [];
    const outputs = ports?.outputs ?? [];
    const portCount = Math.max(inputs.length, outputs.length);

    const allNodes = useNodes() as Node<DuckleNodeData>[];
    const allEdges = useEdges();

    const effectiveSchema = useMemo(
        () => resolveOutputSchema(id, allNodes, allEdges),
        [id, allNodes, allEdges],
    );

    const needsConfig = useMemo(() => {
        if (!manifest) return false;
        const props = data.properties ?? {};
        for (const section of manifest.sections) {
            for (const field of section.fields) {
                if (!field.required) continue;
                const v = props[field.key];
                if (v === undefined || v === null || v === '') return true;
                if (Array.isArray(v) && v.length === 0) return true;
            }
        }
        return false;
    }, [manifest, data.properties]);

    const runStatus = useRunStatus(id);

    const classes =
        'node node-' + kind +
        (selected ? ' is-selected' : '') +
        (data.disabled ? ' is-disabled' : '') +
        (runStatus ? ' is-run-' + runStatus.status : '');

    return (
        <div className={classes}>
            <div className="node-header">
                <div className="node-header-row">
                    <div className="node-kind">{kind}</div>
                    {runStatus ? <RunStatusBadge status={runStatus.status} /> : null}
                    {needsConfig ? (
                        <span
                            className="node-needs-config"
                            title="Required fields missing - open the Basic tab to configure"
                        >
                            <AlertCircle size={12} />
                        </span>
                    ) : null}
                </div>
                <div className="node-label">{data.label}</div>
                {(() => {
                    // Subtitle reflects ONLY the live config (file name,
                    // predicate, group-by keys, …). We intentionally don't
                    // fall back to a seeded subtitle, so a card never shows
                    // a label that isn't in the component's actual config.
                    const subtitle = deriveNodeSubtitle(data.componentId, data.properties);
                    return subtitle ? (
                        <div className="node-subtitle" title={subtitle}>
                            {subtitle}
                        </div>
                    ) : null;
                })()}
                {effectiveSchema.length > 0 ? (
                    <div className="node-schema-badge">
                        {effectiveSchema.length} col{effectiveSchema.length === 1 ? '' : 's'}
                    </div>
                ) : null}
                {data.disabled ? <div className="node-disabled-badge">disabled</div> : null}
            </div>
            {portCount > 0 ? (
                <div className="node-ports">
                    <div className="node-ports-col node-ports-inputs">
                        {inputs.map(port => (
                            <PortRow key={port.id} port={port} side="input" />
                        ))}
                    </div>
                    <div className="node-ports-col node-ports-outputs">
                        {outputs.map(port => (
                            <PortRow key={port.id} port={port} side="output" />
                        ))}
                    </div>
                </div>
            ) : null}
        </div>
    );
}

function RunStatusBadge({ status }: { status: 'running' | 'ok' | 'error' }) {
    if (status === 'running') {
        return (
            <span className="node-run-badge node-run-badge-running" title="Running">
                <Loader2 size={11} />
            </span>
        );
    }
    if (status === 'error') {
        return (
            <span className="node-run-badge node-run-badge-error" title="Failed">
                <XCircle size={11} />
            </span>
        );
    }
    return (
        <span className="node-run-badge node-run-badge-ok" title="OK">
            <CheckCircle2 size={11} />
        </span>
    );
}

function PortRow({ port, side }: { port: PortDef; side: 'input' | 'output' }) {
    const meta = metaFor(port.type);
    const isInput = side === 'input';

    return (
        <div
            className={
                'node-port node-port-' +
                side +
                ' node-port-type-' +
                port.type +
                (port.optional ? ' is-optional' : '')
            }
            title={meta.label + ' · ' + meta.description}
        >
            {isInput ? (
                <Handle
                    type="target"
                    position={Position.Left}
                    id={port.id}
                    className="node-port-handle"
                    style={{ background: meta.color, borderColor: 'var(--bg-1)' }}
                />
            ) : null}
            {isInput ? (
                <>
                    <span
                        className="node-port-dot"
                        style={{ background: meta.color }}
                        aria-hidden="true"
                    />
                    <span className="node-port-label">{port.label}</span>
                </>
            ) : (
                <>
                    <span className="node-port-label">{port.label}</span>
                    <span
                        className="node-port-dot"
                        style={{ background: meta.color }}
                        aria-hidden="true"
                    />
                </>
            )}
            {!isInput ? (
                <Handle
                    type="source"
                    position={Position.Right}
                    id={port.id}
                    className="node-port-handle"
                    style={{ background: meta.color, borderColor: 'var(--bg-1)' }}
                />
            ) : null}
        </div>
    );
}
