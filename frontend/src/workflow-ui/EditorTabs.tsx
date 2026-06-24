import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
    Connection,
    Edge,
    EdgeChange,
    Node,
    NodeChange,
    OnSelectionChangeParams,
} from '@xyflow/react';
import Canvas, { type DropPosition, type NodeAction, type PaneAction } from '../canvas/Canvas';
import PlanView from './PlanView';
import RunView from './RunView';
import RunHistoryView from './RunHistoryView';
import type { EngineId } from './EngineSelector';
import type { DuckleNodeData } from '../pipeline-types';
import type { ComponentDef } from './palette-data';
import type { ConnectionType } from '../canvas/connection-types';
import type { RunResult } from '../tauri-bridge';

type TabId = 'canvas' | 'plan' | 'run' | 'history';

// Tab labels resolved per-render via useTranslation; we keep just the IDs here.
const TAB_IDS: TabId[] = ['canvas', 'plan', 'run', 'history'];

type Props = {
    engine: EngineId;
    nodes: Node<DuckleNodeData>[];
    /** Nodes with ${...} placeholders resolved, for the Plan preview (#105). */
    planNodes: Node<DuckleNodeData>[];
    edges: Edge[];
    runResult: RunResult | null;
    isRunning: boolean;
    nodeLabels: Record<string, string>;
    workspacePath: string | null;
    pipelineId: string | null;
    onNodesChange: (changes: NodeChange[]) => void;
    onEdgesChange: (changes: EdgeChange[]) => void;
    onConnectWithType: (connection: Connection, type: ConnectionType) => void;
    onSelectionChange: (params: OnSelectionChangeParams) => void;
    onDropComponent: (component: ComponentDef, position: DropPosition) => void;
    onSetActiveContext?: (id: string) => void;
    onNodeAction: (action: NodeAction, nodeId: string) => void;
    onPaneAction: (action: PaneAction) => void;
    onEdgeChangeType: (edgeId: string, newType: ConnectionType) => void;
    onEdgeDelete: (edgeId: string) => void;
    onEdgeEdit: (edgeId: string) => void;
    nodeAutodetectAvailable: (nodeId: string) => boolean;
};

export default function EditorTabs({
    engine: _engine,
    nodes,
    edges,
    runResult,
    isRunning,
    planNodes,
    nodeLabels,
    workspacePath,
    pipelineId,
    onNodesChange,
    onEdgesChange,
    onConnectWithType,
    onSelectionChange,
    onDropComponent,
    onSetActiveContext,
    onNodeAction,
    onPaneAction,
    onEdgeChangeType,
    onEdgeDelete,
    onEdgeEdit,
    nodeAutodetectAvailable,
}: Props) {
    const { t } = useTranslation();
    const [active, setActive] = useState<TabId>('canvas');

    return (
        <div className="editor">
            <div className="tabbar" role="tablist" aria-label={t('editorTabs.ariaLabel')}>
                {TAB_IDS.map(id => (
                    <button
                        key={id}
                        type="button"
                        role="tab"
                        aria-selected={active === id}
                        className="tab"
                        onClick={() => setActive(id)}
                    >
                        {/* `history` is a newer tab not yet in every locale file;
                            fall back to a readable English label when missing. */}
                        {t(`editorTabs.${id}`, { defaultValue: id === 'history' ? 'History' : id })}
                    </button>
                ))}
            </div>
            <div className="tab-content">
                <div className={'tab-panel' + (active === 'canvas' ? ' tab-panel-active' : '')}>
                    <Canvas
                        nodes={nodes}
                        edges={edges}
                        pipelineId={pipelineId}
                        onNodesChange={onNodesChange}
                        onEdgesChange={onEdgesChange}
                        onConnectWithType={onConnectWithType}
                        onSelectionChange={onSelectionChange}
                        onDropComponent={onDropComponent}
                        onSetActiveContext={onSetActiveContext}
                        onNodeAction={onNodeAction}
                        onPaneAction={onPaneAction}
                        onEdgeChangeType={onEdgeChangeType}
                        onEdgeDelete={onEdgeDelete}
                        onEdgeEdit={onEdgeEdit}
                        nodeAutodetectAvailable={nodeAutodetectAvailable}
                    />
                </div>
                <div className={'tab-panel' + (active === 'plan' ? ' tab-panel-active' : '')}>
                    <PlanView nodes={planNodes} edges={edges} />
                </div>
                <div className={'tab-panel' + (active === 'run' ? ' tab-panel-active' : '')}>
                    <RunView
                        runResult={runResult}
                        isRunning={isRunning}
                        nodeLabels={nodeLabels}
                    />
                </div>
                <div className={'tab-panel' + (active === 'history' ? ' tab-panel-active' : '')}>
                    {/* Mount lazily so history only loads when the tab is opened;
                        runResult identity changes after each run, triggering a reload. */}
                    {active === 'history' ? (
                        <RunHistoryView
                            workspacePath={workspacePath}
                            pipelineId={pipelineId}
                            runResultKey={runResult?.duration_ms ?? 0}
                        />
                    ) : null}
                </div>
            </div>
        </div>
    );
}
