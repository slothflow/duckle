// Column-level lineage viewer (#103). Calls the engine's pipeline_column_lineage
// for the active pipeline and shows, per node, each output column traced back to
// the source column(s) it derives from. Read-only.

import { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import type { Edge, Node } from '@xyflow/react';
import type { DuckleNodeData } from '../pipeline-types';
import { pipelineColumnLineage, type PipelineLineage } from '../tauri-bridge';

interface LineageModalProps {
    nodes: Node<DuckleNodeData>[];
    edges: Edge[];
    onClose: () => void;
}

export function LineageModal({ nodes, edges, onClose }: LineageModalProps) {
    const [data, setData] = useState<PipelineLineage | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        let cancelled = false;
        void (async () => {
            try {
                const r = await pipelineColumnLineage(nodes, edges);
                if (!cancelled) {
                    setData(r);
                    setLoading(false);
                }
            } catch (e) {
                if (!cancelled) {
                    setError(e instanceof Error ? e.message : String(e));
                    setLoading(false);
                }
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [nodes, edges]);

    const labelOf = (id: string) => nodes.find((n) => n.id === id)?.data?.label ?? id;
    const entries = data ? Object.entries(data).filter(([, cols]) => cols.length > 0) : [];

    return (
        <div className="dive-modal-backdrop" onClick={onClose}>
            <div className="lineage-modal" onClick={(e) => e.stopPropagation()}>
                <div className="lineage-head">
                    <h2 className="lineage-title">Column lineage</h2>
                    <button type="button" className="dive-btn" onClick={onClose} aria-label="Close">
                        <X size={16} />
                    </button>
                </div>
                <p className="lineage-sub">
                    Each output column traced back to the source column(s) it derives from.
                </p>
                {loading ? <div className="dive-panel-msg">Resolving lineage…</div> : null}
                {error ? <div className="dive-panel-msg dive-panel-err">{error}</div> : null}
                {!loading && !error ? (
                    entries.length === 0 ? (
                        <div className="dive-panel-msg">
                            No column lineage to show yet. Add a transform or sink whose SQL selects columns.
                        </div>
                    ) : (
                        <div className="lineage-body">
                            {entries.map(([nodeId, cols]) => (
                                <div key={nodeId} className="lineage-node">
                                    <div className="lineage-node-name">{labelOf(nodeId)}</div>
                                    <table className="lineage-table">
                                        <thead>
                                            <tr>
                                                <th>Column</th>
                                                <th>From</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {cols.map(([col, roots], i) => (
                                                <tr key={i}>
                                                    <td className="lineage-col">{col}</td>
                                                    <td className="lineage-roots">
                                                        {roots.length === 0 ? (
                                                            <span className="lineage-none">-</span>
                                                        ) : (
                                                            roots.map((r, j) => (
                                                                <span key={j} className="lineage-root">
                                                                    {labelOf(r.node)}.{r.column}
                                                                </span>
                                                            ))
                                                        )}
                                                    </td>
                                                </tr>
                                            ))}
                                        </tbody>
                                    </table>
                                </div>
                            ))}
                        </div>
                    )
                ) : null}
            </div>
        </div>
    );
}
