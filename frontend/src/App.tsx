import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
    addEdge,
    applyEdgeChanges,
    applyNodeChanges,
    type Connection,
    type Edge,
    type EdgeChange,
    type Node,
    type NodeChange,
    type OnSelectionChangeParams,
} from '@xyflow/react';
import type { ConnectionType } from './canvas/connection-types';
import EditorTabs from './workflow-ui/EditorTabs';
import EditorHeader, { type Job } from './workflow-ui/EditorHeader';
import EngineSelector, { type EngineId } from './workflow-ui/EngineSelector';
import LeftSidebar from './workflow-ui/LeftSidebar';
import PropertiesPanel from './workflow-ui/PropertiesPanel';
import BottomPanel from './workflow-ui/BottomPanel';
import StatusBar from './workflow-ui/StatusBar';
import NewPipelineModal, { type PipelineTemplate } from './workflow-ui/NewPipelineModal';
import type { ComponentDef, NodeKind as PaletteKind } from './workflow-ui/palette-data';
import { getDefaults, getManifest } from './workflow-ui/fields/component-manifests';
import type { DuckleNodeData } from './pipeline-types';
import type { DropPosition, NodeAction, PaneAction } from './canvas/Canvas';
import type { RepoItem } from './repo-types';

type RuntimeState = 'connecting' | 'ready' | 'offline';

type PipelineState = {
    nodes: Node<DuckleNodeData>[];
    edges: Edge[];
};

const SAMPLE_NODES: Node<DuckleNodeData>[] = [
    {
        id: 's1',
        type: 'source',
        position: { x: 60, y: 140 },
        data: {
            label: 'CSV',
            subtitle: 'orders.csv',
            componentId: 'src.csv',
            schema: [
                { name: 'order_id', type: 'int64', nullable: false, primaryKey: true },
                { name: 'customer_id', type: 'int64', nullable: false },
                { name: 'status', type: 'string', nullable: false },
                { name: 'amount', type: 'decimal', nullable: true },
                { name: 'created_at', type: 'timestamp', nullable: false },
            ],
        },
    },
    {
        id: 't1',
        type: 'transform',
        position: { x: 340, y: 140 },
        data: {
            label: 'Filter',
            subtitle: 'status = "paid"',
            componentId: 'xf.filter',
        },
    },
    {
        id: 'k1',
        type: 'sink',
        position: { x: 620, y: 140 },
        data: {
            label: 'Parquet',
            subtitle: 'orders_paid.parquet',
            componentId: 'snk.parquet',
        },
    },
];

const SAMPLE_EDGES: Edge[] = [
    {
        id: 'e1',
        source: 's1',
        sourceHandle: 'main',
        target: 't1',
        targetHandle: 'main',
        type: 'duckle',
        data: { connectionType: 'main' },
    },
    {
        id: 'e2',
        source: 't1',
        sourceHandle: 'main',
        target: 'k1',
        targetHandle: 'main',
        type: 'duckle',
        data: { connectionType: 'main' },
    },
];

const INITIAL_JOBS: Job[] = [{ id: 'j1', name: 'orders_etl', dirty: false }];

const INITIAL_PIPELINE_DATA: Record<string, PipelineState> = {
    j1: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
};

const INITIAL_REPO: RepoItem[] = [
    { id: 'root', name: 'Duckle Project', type: 'project' },
    { id: 'pipelines', name: 'Pipelines', type: 'folder', parentId: 'root' },
    { id: 'connections', name: 'Connections', type: 'folder', parentId: 'root' },
    { id: 'contexts', name: 'Contexts', type: 'folder', parentId: 'root' },
    { id: 'routines', name: 'Routines', type: 'folder', parentId: 'root' },
    { id: 'docs', name: 'Documentation', type: 'folder', parentId: 'root' },
    { id: 'j1', name: 'orders_etl', type: 'pipeline', parentId: 'pipelines' },
];

function paletteKindToFlowType(kind: PaletteKind): string {
    switch (kind) {
        case 'source':
            return 'source';
        case 'sink':
            return 'sink';
        case 'transform':
        case 'control':
        case 'quality':
        case 'custom':
            return 'transform';
    }
}

function freshId(prefix: string): string {
    return prefix + '_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 7);
}

function seedTemplate(template: PipelineTemplate): PipelineState {
    if (template === 'sample-csv-to-parquet') {
        return { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES };
    }
    return { nodes: [], edges: [] };
}

const EMPTY_PIPELINE: PipelineState = { nodes: [], edges: [] };

export default function App() {
    const [runtime, setRuntime] = useState<RuntimeState>('connecting');
    const [engine, setEngine] = useState<EngineId>('duckdb');
    const [pipelineData, setPipelineData] =
        useState<Record<string, PipelineState>>(INITIAL_PIPELINE_DATA);
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [jobs, setJobs] = useState<Job[]>(INITIAL_JOBS);
    const [activeJobId, setActiveJobId] = useState<string>('j1');
    const [isRunning, setIsRunning] = useState<boolean>(false);
    const [renameRequest, setRenameRequest] = useState<number>(0);
    const [repo, setRepo] = useState<RepoItem[]>(INITIAL_REPO);
    const [newPipelineModal, setNewPipelineModal] = useState<{
        open: boolean;
        defaultParent: string;
    }>({ open: false, defaultParent: 'pipelines' });

    const activePipeline = pipelineData[activeJobId] ?? EMPTY_PIPELINE;
    const nodes = activePipeline.nodes;
    const edges = activePipeline.edges;

    useEffect(() => {
        let cancelled = false;
        invoke<string>('ping')
            .then(reply => {
                if (!cancelled) setRuntime(reply === 'pong' ? 'ready' : 'offline');
            })
            .catch(() => {
                if (!cancelled) setRuntime('offline');
            });
        return () => {
            cancelled = true;
        };
    }, []);

    // Switching active pipeline resets node selection.
    useEffect(() => {
        setSelectedId(null);
    }, [activeJobId]);

    const updateActive = useCallback(
        (updater: (s: PipelineState) => PipelineState) => {
            setPipelineData(d => ({
                ...d,
                [activeJobId]: updater(d[activeJobId] ?? EMPTY_PIPELINE),
            }));
        },
        [activeJobId],
    );

    const setNodes = useCallback(
        (updater: Node<DuckleNodeData>[] | ((ns: Node<DuckleNodeData>[]) => Node<DuckleNodeData>[])) => {
            updateActive(s => ({
                ...s,
                nodes: typeof updater === 'function' ? (updater as (ns: Node<DuckleNodeData>[]) => Node<DuckleNodeData>[])(s.nodes) : updater,
            }));
        },
        [updateActive],
    );

    const setEdges = useCallback(
        (updater: Edge[] | ((es: Edge[]) => Edge[])) => {
            updateActive(s => ({
                ...s,
                edges: typeof updater === 'function' ? (updater as (es: Edge[]) => Edge[])(s.edges) : updater,
            }));
        },
        [updateActive],
    );

    const markDirty = useCallback(() => {
        setJobs(js => js.map(j => (j.id === activeJobId ? { ...j, dirty: true } : j)));
    }, [activeJobId]);

    const handleNodesChange = useCallback(
        (changes: NodeChange[]) => {
            setNodes(ns => applyNodeChanges(changes, ns) as Node<DuckleNodeData>[]);
        },
        [setNodes],
    );

    const handleEdgesChange = useCallback(
        (changes: EdgeChange[]) => {
            setEdges(es => applyEdgeChanges(changes, es));
        },
        [setEdges],
    );

    const handleConnectWithType = useCallback(
        (connection: Connection, type: ConnectionType) => {
            setEdges(es =>
                addEdge(
                    {
                        ...connection,
                        type: 'duckle',
                        data: { connectionType: type },
                    },
                    es,
                ),
            );
            markDirty();
        },
        [setEdges, markDirty],
    );

    const handleEdgeChangeType = useCallback(
        (edgeId: string, newType: ConnectionType) => {
            setEdges(es =>
                es.map(e =>
                    e.id === edgeId
                        ? {
                              ...e,
                              type: 'duckle',
                              data: { ...(e.data ?? {}), connectionType: newType },
                          }
                        : e,
                ),
            );
            markDirty();
        },
        [setEdges, markDirty],
    );

    const handleEdgeDelete = useCallback(
        (edgeId: string) => {
            setEdges(es => es.filter(e => e.id !== edgeId));
            markDirty();
        },
        [setEdges, markDirty],
    );

    const handleSelectionChange = useCallback((params: OnSelectionChangeParams) => {
        setSelectedId(params.nodes[0]?.id ?? null);
    }, []);

    const handleUpdateNode = useCallback(
        (id: string, patch: Partial<DuckleNodeData>) => {
            setNodes(ns =>
                ns.map(n => (n.id === id ? { ...n, data: { ...n.data, ...patch } } : n)),
            );
            markDirty();
        },
        [setNodes, markDirty],
    );

    const selectedNode = useMemo(
        () => nodes.find(n => n.id === selectedId) ?? null,
        [nodes, selectedId],
    );

    const openNewPipelineModal = useCallback((parentId: string = 'pipelines') => {
        setNewPipelineModal({ open: true, defaultParent: parentId });
    }, []);

    const handleNewJob = useCallback(() => {
        openNewPipelineModal('pipelines');
    }, [openNewPipelineModal]);

    const handleCloseJob = useCallback(
        (id: string) => {
            setJobs(js => js.filter(j => j.id !== id));
            if (activeJobId === id) {
                const remaining = jobs.filter(j => j.id !== id);
                setActiveJobId(remaining[0]?.id ?? '');
            }
        },
        [activeJobId, jobs],
    );

    const handleRun = useCallback(() => {
        setIsRunning(true);
        setTimeout(() => setIsRunning(false), 2000);
    }, []);

    const handleStop = useCallback(() => setIsRunning(false), []);

    const handleSave = useCallback(() => {
        setJobs(js => js.map(j => (j.id === activeJobId ? { ...j, dirty: false } : j)));
    }, [activeJobId]);

    const handleValidate = useCallback(() => {
        // Real validation lands in Option B.
    }, []);

    const handleAutoLayout = useCallback(() => {
        setNodes(ns =>
            ns.map((n, i) => ({
                ...n,
                position: { x: 60 + i * 280, y: 140 },
            })),
        );
    }, [setNodes]);

    const handleDropComponent = useCallback(
        (component: ComponentDef, position: DropPosition) => {
            const id = freshId('n');
            const manifest = getManifest(component.id);
            const flowType = paletteKindToFlowType(component.kind);
            const newNode: Node<DuckleNodeData> = {
                id,
                type: flowType,
                position,
                data: {
                    label: component.label,
                    subtitle: component.summary,
                    componentId: component.id,
                    properties: manifest ? getDefaults(manifest) : {},
                },
            };
            setNodes(ns => [...ns, newNode]);
            setSelectedId(id);
            markDirty();

            // Auto-detect schema for sources / autodetect-capable components
            // so downstream nodes inherit immediately. The mock returns sample
            // columns; real autodetect lands when the runtime can read files.
            if (manifest?.autodetect) {
                void manifest.autodetect().then(result => {
                    setNodes(ns =>
                        ns.map(n =>
                            n.id === id
                                ? {
                                      ...n,
                                      data: {
                                          ...n.data,
                                          schema: result.columns,
                                          sampleRows: result.sampleRows,
                                      },
                                  }
                                : n,
                        ),
                    );
                });
            }
        },
        [setNodes, markDirty],
    );

    const nodeAutodetectAvailable = useCallback(
        (nodeId: string) => {
            const node = nodes.find(n => n.id === nodeId);
            if (!node) return false;
            const manifest = getManifest(node.data.componentId);
            return Boolean(manifest?.autodetect);
        },
        [nodes],
    );

    const handleNodeAction = useCallback(
        (action: NodeAction, nodeId: string) => {
            const node = nodes.find(n => n.id === nodeId);
            if (!node) return;

            switch (action) {
                case 'rename':
                    setSelectedId(nodeId);
                    setRenameRequest(n => n + 1);
                    break;

                case 'duplicate': {
                    const dupId = freshId('n');
                    const copy: Node<DuckleNodeData> = {
                        ...node,
                        id: dupId,
                        position: { x: node.position.x + 40, y: node.position.y + 40 },
                        data: { ...node.data, label: node.data.label + ' (copy)' },
                        selected: false,
                    };
                    setNodes(ns => [...ns, copy]);
                    setSelectedId(dupId);
                    markDirty();
                    break;
                }

                case 'toggle-disable':
                    setNodes(ns =>
                        ns.map(n =>
                            n.id === nodeId
                                ? {
                                      ...n,
                                      data: { ...n.data, disabled: !n.data.disabled },
                                  }
                                : n,
                        ),
                    );
                    markDirty();
                    break;

                case 'autodetect': {
                    const manifest = getManifest(node.data.componentId);
                    if (!manifest?.autodetect) return;
                    void manifest.autodetect().then(result => {
                        setNodes(ns =>
                            ns.map(n =>
                                n.id === nodeId
                                    ? {
                                          ...n,
                                          data: {
                                              ...n.data,
                                              schema: result.columns,
                                              sampleRows: result.sampleRows,
                                          },
                                      }
                                    : n,
                            ),
                        );
                        markDirty();
                    });
                    break;
                }

                case 'run-from-here':
                    break;

                case 'copy-id':
                    void navigator.clipboard?.writeText(nodeId);
                    break;

                case 'delete':
                    setNodes(ns => ns.filter(n => n.id !== nodeId));
                    setEdges(es => es.filter(e => e.source !== nodeId && e.target !== nodeId));
                    if (selectedId === nodeId) setSelectedId(null);
                    markDirty();
                    break;
            }
        },
        [nodes, selectedId, setNodes, setEdges, markDirty],
    );

    const handlePaneAction = useCallback(
        (action: PaneAction) => {
            switch (action) {
                case 'auto-layout':
                    handleAutoLayout();
                    break;
                case 'select-all':
                    setNodes(ns => ns.map(n => ({ ...n, selected: true })));
                    break;
                case 'paste':
                    break;
            }
        },
        [handleAutoLayout, setNodes],
    );

    // Repository handlers ---------------------------------------------------
    const handleOpenPipeline = useCallback(
        (id: string) => {
            const item = repo.find(i => i.id === id);
            if (!item || item.type !== 'pipeline') return;
            setJobs(js =>
                js.find(j => j.id === id) ? js : [...js, { id, name: item.name, dirty: false }],
            );
            setPipelineData(d => (d[id] ? d : { ...d, [id]: EMPTY_PIPELINE }));
            setActiveJobId(id);
        },
        [repo],
    );

    const handleNewFolderInRepo = useCallback(
        (parentId: string) => {
            const id = 'f_' + Date.now().toString(36);
            const count = repo.filter(i => i.type === 'folder' && i.parentId === parentId).length;
            const name = 'new_folder' + (count > 0 ? '_' + (count + 1) : '');
            const realParent = repo.find(
                i => i.id === parentId && (i.type === 'folder' || i.type === 'project'),
            )
                ? parentId
                : 'root';
            setRepo(r => [...r, { id, name, type: 'folder', parentId: realParent }]);
        },
        [repo],
    );

    const handleRenameRepoItem = useCallback((id: string, newName: string) => {
        setRepo(r => r.map(i => (i.id === id ? { ...i, name: newName } : i)));
        setJobs(js => js.map(j => (j.id === id ? { ...j, name: newName } : j)));
    }, []);

    const handleDuplicateRepoItem = useCallback(
        (id: string) => {
            const item = repo.find(i => i.id === id);
            if (!item) return;
            const newId = item.type[0] + '_' + Date.now().toString(36);
            setRepo(r => [...r, { ...item, id: newId, name: item.name + '_copy' }]);
            if (item.type === 'pipeline') {
                setPipelineData(d => ({ ...d, [newId]: d[id] ?? EMPTY_PIPELINE }));
            }
        },
        [repo],
    );

    const handleDeleteRepoItem = useCallback(
        (id: string) => {
            const item = repo.find(i => i.id === id);
            if (!item || item.type === 'project') return;
            const toDelete = new Set<string>([id]);
            const addDescendants = (parentId: string) => {
                for (const c of repo) {
                    if (c.parentId === parentId) {
                        toDelete.add(c.id);
                        addDescendants(c.id);
                    }
                }
            };
            addDescendants(id);
            setRepo(r => r.filter(i => !toDelete.has(i.id)));
            setJobs(js => js.filter(j => !toDelete.has(j.id)));
            setPipelineData(d => {
                const next = { ...d };
                for (const did of toDelete) delete next[did];
                return next;
            });
            if (toDelete.has(activeJobId)) {
                const remaining = jobs.filter(j => !toDelete.has(j.id));
                setActiveJobId(remaining[0]?.id ?? '');
            }
        },
        [repo, jobs, activeJobId],
    );

    const handleCreatePipeline = useCallback(
        (rawName: string, parentId: string, template: PipelineTemplate) => {
            const id = freshId('p');
            const realParent = repo.find(
                i => i.id === parentId && (i.type === 'folder' || i.type === 'project'),
            )
                ? parentId
                : 'pipelines';
            const seed = seedTemplate(template);
            setRepo(r => [...r, { id, name: rawName, type: 'pipeline', parentId: realParent }]);
            setPipelineData(d => ({ ...d, [id]: seed }));
            setJobs(js => [...js, { id, name: rawName, dirty: false }]);
            setActiveJobId(id);
            setNewPipelineModal({ open: false, defaultParent: 'pipelines' });
        },
        [repo],
    );

    const openJobIds = useMemo(() => new Set(jobs.map(j => j.id)), [jobs]);

    return (
        <div className="app">
            <header className="topbar">
                <div className="brand">
                    <span className="brand-mark" aria-hidden="true">
                        D
                    </span>
                    Duckle
                </div>
                <div className="topbar-sep" aria-hidden="true" />
                <EngineSelector value={engine} onChange={setEngine} />
                <div className="topbar-spacer" />
                <div className="status" data-state={runtime}>
                    <span className="status-dot" /> runtime: {runtime}
                </div>
            </header>

            <main className="workspace">
                <LeftSidebar
                    repoItems={repo}
                    activeJobId={activeJobId}
                    openJobIds={openJobIds}
                    onOpenPipeline={handleOpenPipeline}
                    onNewPipeline={openNewPipelineModal}
                    onNewFolder={handleNewFolderInRepo}
                    onRenameRepoItem={handleRenameRepoItem}
                    onDuplicateRepoItem={handleDuplicateRepoItem}
                    onDeleteRepoItem={handleDeleteRepoItem}
                />
                <section className="canvas-shell">
                    <EditorHeader
                        jobs={jobs}
                        activeJobId={activeJobId}
                        isRunning={isRunning}
                        onSelectJob={setActiveJobId}
                        onCloseJob={handleCloseJob}
                        onNewJob={handleNewJob}
                        onRun={handleRun}
                        onStop={handleStop}
                        onSave={handleSave}
                        onValidate={handleValidate}
                        onAutoLayout={handleAutoLayout}
                    />
                    <EditorTabs
                        engine={engine}
                        nodes={nodes}
                        edges={edges}
                        onNodesChange={handleNodesChange}
                        onEdgesChange={handleEdgesChange}
                        onConnectWithType={handleConnectWithType}
                        onSelectionChange={handleSelectionChange}
                        onDropComponent={handleDropComponent}
                        onNodeAction={handleNodeAction}
                        onPaneAction={handlePaneAction}
                        onEdgeChangeType={handleEdgeChangeType}
                        onEdgeDelete={handleEdgeDelete}
                        nodeAutodetectAvailable={nodeAutodetectAvailable}
                    />
                </section>
                <PropertiesPanel
                    selected={selectedNode}
                    allNodes={nodes}
                    edges={edges}
                    onUpdate={handleUpdateNode}
                    focusNameRequest={renameRequest}
                />
            </main>

            <BottomPanel />

            <StatusBar
                engine={engine}
                runtime={runtime}
                nodeCount={nodes.length}
                edgeCount={edges.length}
            />

            <NewPipelineModal
                open={newPipelineModal.open}
                defaultParentId={newPipelineModal.defaultParent}
                repoItems={repo}
                onCancel={() =>
                    setNewPipelineModal({ open: false, defaultParent: 'pipelines' })
                }
                onCreate={handleCreatePipeline}
            />
        </div>
    );
}
