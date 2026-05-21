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
import EditorTabs from './workflow-ui/EditorTabs';
import EditorHeader, { type Job } from './workflow-ui/EditorHeader';
import EngineSelector, { type EngineId } from './workflow-ui/EngineSelector';
import Palette from './workflow-ui/Palette';
import PropertiesPanel from './workflow-ui/PropertiesPanel';
import BottomPanel from './workflow-ui/BottomPanel';
import StatusBar from './workflow-ui/StatusBar';
import type { DuckleNodeData } from './pipeline-types';

type RuntimeState = 'connecting' | 'ready' | 'offline';

const INITIAL_NODES: Node<DuckleNodeData>[] = [
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

const INITIAL_EDGES: Edge[] = [
    { id: 'e1', source: 's1', target: 't1' },
    { id: 'e2', source: 't1', target: 'k1' },
];

const INITIAL_JOBS: Job[] = [{ id: 'j1', name: 'orders_etl', dirty: false }];

export default function App() {
    const [runtime, setRuntime] = useState<RuntimeState>('connecting');
    const [engine, setEngine] = useState<EngineId>('duckdb');
    const [nodes, setNodes] = useState<Node<DuckleNodeData>[]>(INITIAL_NODES);
    const [edges, setEdges] = useState<Edge[]>(INITIAL_EDGES);
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [jobs, setJobs] = useState<Job[]>(INITIAL_JOBS);
    const [activeJobId, setActiveJobId] = useState<string>('j1');
    const [isRunning, setIsRunning] = useState<boolean>(false);

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

    const handleNodesChange = useCallback((changes: NodeChange[]) => {
        setNodes(ns => applyNodeChanges(changes, ns) as Node<DuckleNodeData>[]);
    }, []);

    const handleEdgesChange = useCallback((changes: EdgeChange[]) => {
        setEdges(es => applyEdgeChanges(changes, es));
    }, []);

    const handleConnect = useCallback((connection: Connection) => {
        setEdges(es => addEdge(connection, es));
    }, []);

    const handleSelectionChange = useCallback((params: OnSelectionChangeParams) => {
        setSelectedId(params.nodes[0]?.id ?? null);
    }, []);

    const handleUpdateNode = useCallback((id: string, patch: Partial<DuckleNodeData>) => {
        setNodes(ns =>
            ns.map(n => (n.id === id ? { ...n, data: { ...n.data, ...patch } } : n)),
        );
    }, []);

    const selectedNode = useMemo(
        () => nodes.find(n => n.id === selectedId) ?? null,
        [nodes, selectedId],
    );

    const handleNewJob = useCallback(() => {
        const id = 'j' + (jobs.length + 1);
        setJobs(js => [...js, { id, name: 'untitled-' + (js.length + 1), dirty: false }]);
        setActiveJobId(id);
    }, [jobs.length]);

    const handleCloseJob = useCallback(
        (id: string) => {
            setJobs(js => js.filter(j => j.id !== id));
            if (activeJobId === id) {
                setActiveJobId(jobs[0]?.id ?? '');
            }
        },
        [activeJobId, jobs],
    );

    const handleRun = useCallback(() => {
        setIsRunning(true);
        // Real execution wires up in Option C.
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
        // Real layout solver lands later; basic horizontal stack for now.
        setNodes(ns =>
            ns.map((n, i) => ({
                ...n,
                position: { x: 60 + i * 280, y: 140 },
            })),
        );
    }, []);

    return (
        <div className="app">
            <header className="topbar">
                <div className="brand">
                    <span className="brand-mark">◇</span> Duckle
                </div>
                <div className="topbar-sep" aria-hidden="true" />
                <EngineSelector value={engine} onChange={setEngine} />
                <div className="topbar-spacer" />
                <div className="status" data-state={runtime}>
                    <span className="status-dot" /> runtime: {runtime}
                </div>
            </header>

            <main className="workspace">
                <Palette />
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
                        onConnect={handleConnect}
                        onSelectionChange={handleSelectionChange}
                    />
                </section>
                <PropertiesPanel
                    selected={selectedNode}
                    allNodes={nodes}
                    edges={edges}
                    onUpdate={handleUpdateNode}
                />
            </main>

            <BottomPanel />

            <StatusBar
                engine={engine}
                runtime={runtime}
                nodeCount={nodes.length}
                edgeCount={edges.length}
            />
        </div>
    );
}
