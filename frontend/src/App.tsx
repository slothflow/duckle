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
import { Moon, Sun } from 'lucide-react';
import EditorTabs from './workflow-ui/EditorTabs';
import EditorHeader, { type Job } from './workflow-ui/EditorHeader';
import EngineSelector, { type EngineId } from './workflow-ui/EngineSelector';
import { useTheme } from './theme';
import { loadPersisted, savePersisted } from './persistence';
import { resolveOutputSchema } from './schema-resolve';
import {
    cancelPipeline,
    runPipeline,
    runPipelinePartial,
    type PipelineEvent,
    type RunResult,
} from './tauri-bridge';
import { RunStatusContext } from './canvas/run-status-context';
import WorkspacePickerModal from './workflow-ui/WorkspacePickerModal';
import {
    getWorkspacePath,
    isInTauri,
    loadWorkspace,
    saveWorkspace,
    setWorkspacePath,
} from './workspace';
import LeftSidebar from './workflow-ui/LeftSidebar';
import PropertiesPanel from './workflow-ui/PropertiesPanel';
import BottomPanel from './workflow-ui/BottomPanel';
import StatusBar from './workflow-ui/StatusBar';
import NewPipelineModal, { type PipelineTemplate } from './workflow-ui/NewPipelineModal';
import EdgeEditorModal from './canvas/EdgeEditorModal';
import VisualMapperModal, {
    type MapperState,
    type MappingRow,
} from './canvas/VisualMapperModal';
import ConnectionEditorModal from './workflow-ui/editors/ConnectionEditorModal';
import ContextEditorModal from './workflow-ui/editors/ContextEditorModal';
import DocumentEditorModal from './workflow-ui/editors/DocumentEditorModal';
import RoutineEditorModal from './workflow-ui/editors/RoutineEditorModal';
import type { Column } from './pipeline-types';
import type { ComponentDef, NodeKind as PaletteKind } from './workflow-ui/palette-data';
import type {
    ConnectionPayload,
    ContextPayload,
    DocumentPayload,
    RoutinePayload,
} from './repo-types';
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
    const { theme, toggle: toggleTheme } = useTheme();
    const [runtime, setRuntime] = useState<RuntimeState>('connecting');
    const [engine, setEngine] = useState<EngineId>(() =>
        loadPersisted<EngineId>('engine', 'duckdb'),
    );
    const [pipelineData, setPipelineData] = useState<Record<string, PipelineState>>(() =>
        loadPersisted('pipelines', INITIAL_PIPELINE_DATA),
    );
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [jobs, setJobs] = useState<Job[]>(() => loadPersisted('jobs', INITIAL_JOBS));
    const [activeJobId, setActiveJobId] = useState<string>(() =>
        loadPersisted('active-job', 'j1'),
    );
    const [isRunning, setIsRunning] = useState<boolean>(false);
    const [renameRequest, setRenameRequest] = useState<number>(0);
    const [repo, setRepo] = useState<RepoItem[]>(() => loadPersisted('repo', INITIAL_REPO));

    const [workspacePathState, setWorkspacePathState] = useState<string | null>(() =>
        getWorkspacePath(),
    );
    // In Tauri: needs workspace picked + hydrated before saves start.
    // In browser: workspaceReady is always true; localStorage persists.
    const [workspaceReady, setWorkspaceReady] = useState<boolean>(!isInTauri());
    const showWorkspacePicker = isInTauri() && !workspacePathState;
    const [newPipelineModal, setNewPipelineModal] = useState<{
        open: boolean;
        defaultParent: string;
    }>({ open: false, defaultParent: 'pipelines' });

    const activePipeline = pipelineData[activeJobId] ?? EMPTY_PIPELINE;
    const nodes = activePipeline.nodes;
    const edges = activePipeline.edges;

    // Hydrate from workspace file on Tauri once the path is known.
    useEffect(() => {
        if (!isInTauri() || !workspacePathState) return;
        let cancelled = false;
        loadWorkspace(workspacePathState).then(state => {
            if (cancelled) return;
            if (state) {
                if (state.engine) setEngine(state.engine as EngineId);
                if (state.pipelineData)
                    setPipelineData(state.pipelineData as Record<string, PipelineState>);
                if (state.repo) setRepo(state.repo as RepoItem[]);
                if (state.jobs) setJobs(state.jobs as Job[]);
                if (state.activeJobId) setActiveJobId(state.activeJobId);
            }
            setWorkspaceReady(true);
        });
        return () => {
            cancelled = true;
        };
    }, [workspacePathState]);

    // Tauri: save to disk (debounced). Browser: save to localStorage.
    useEffect(() => {
        if (!workspaceReady) return;
        if (isInTauri() && workspacePathState) {
            const t = setTimeout(() => {
                void saveWorkspace(workspacePathState, {
                    version: 1,
                    engine,
                    pipelineData: pipelineData as unknown as Record<string, unknown>,
                    repo: repo as unknown[],
                    jobs: jobs as unknown[],
                    activeJobId,
                });
            }, 500);
            return () => clearTimeout(t);
        } else {
            const t = setTimeout(() => {
                savePersisted('pipelines', pipelineData);
                savePersisted('repo', repo);
                savePersisted('jobs', jobs);
                savePersisted('active-job', activeJobId);
                savePersisted('engine', engine);
            }, 250);
            return () => clearTimeout(t);
        }
    }, [
        workspaceReady,
        workspacePathState,
        pipelineData,
        repo,
        jobs,
        activeJobId,
        engine,
    ]);

    const handlePickedWorkspace = useCallback((path: string) => {
        setWorkspacePath(path);
        setWorkspacePathState(path);
    }, []);

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

            // Auto-populate the right-side key on join/lookup components
            // when a lookup connection lands on them — picks up the
            // first column of the lookup source's effective schema.
            if (type === 'lookup' && connection.target && connection.source) {
                const targetNode = nodes.find(n => n.id === connection.target);
                const targetManifest = targetNode
                    ? getManifest(targetNode.data.componentId)
                    : undefined;
                const targetId = targetManifest?.id ?? '';
                const isJoinFamily =
                    targetId.startsWith('xf.join.') ||
                    targetId === 'xf.lookup' ||
                    targetId === 'xf.semi' ||
                    targetId === 'xf.anti';
                if (isJoinFamily && targetNode && !targetNode.data.properties?.rightKey) {
                    const lookupSchema = resolveOutputSchema(connection.source, nodes, edges);
                    const firstCol = lookupSchema[0]?.name;
                    if (firstCol) {
                        setNodes(ns =>
                            ns.map(n =>
                                n.id === connection.target
                                    ? {
                                          ...n,
                                          data: {
                                              ...n.data,
                                              properties: {
                                                  ...(n.data.properties ?? {}),
                                                  rightKey: firstCol,
                                              },
                                          },
                                      }
                                    : n,
                            ),
                        );
                    }
                }
            }

            markDirty();
        },
        [nodes, edges, setNodes, setEdges, markDirty],
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

    const [mapperNodeId, setMapperNodeId] = useState<string | null>(null);
    const mapperNode = useMemo(
        () => (mapperNodeId ? nodes.find(n => n.id === mapperNodeId) ?? null : null),
        [mapperNodeId, nodes],
    );
    const handleOpenMapper = useCallback((nodeId: string) => {
        setMapperNodeId(nodeId);
    }, []);
    const handleMapperSave = useCallback(
        (state: MapperState, derivedSchema: Column[]) => {
            if (!mapperNodeId) return;
            setNodes(ns =>
                ns.map(n =>
                    n.id === mapperNodeId
                        ? {
                              ...n,
                              data: {
                                  ...n.data,
                                  properties: {
                                      ...(n.data.properties ?? {}),
                                      mapper: state as unknown as Record<string, unknown>,
                                      mode: 'visual',
                                  },
                                  schema: derivedSchema,
                              },
                          }
                        : n,
                ),
            );
            setMapperNodeId(null);
            markDirty();
        },
        [mapperNodeId, setNodes, markDirty],
    );

    const [editingEdgeId, setEditingEdgeId] = useState<string | null>(null);
    const editingEdge = useMemo(
        () => (editingEdgeId ? edges.find(e => e.id === editingEdgeId) ?? null : null),
        [editingEdgeId, edges],
    );

    const handleEdgeEdit = useCallback((edgeId: string) => {
        setEditingEdgeId(edgeId);
    }, []);

    const handleEdgeEditSave = useCallback(
        (patch: { label?: string; condition?: string }) => {
            if (!editingEdgeId) return;
            setEdges(es =>
                es.map(e =>
                    e.id === editingEdgeId
                        ? {
                              ...e,
                              data: {
                                  ...(e.data ?? {}),
                                  ...(patch.label !== undefined ? { label: patch.label } : {}),
                                  ...(patch.condition !== undefined
                                      ? { condition: patch.condition }
                                      : {}),
                              },
                          }
                        : e,
                ),
            );
            setEditingEdgeId(null);
            markDirty();
        },
        [editingEdgeId, setEdges, markDirty],
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

    const [runResult, setRunResult] = useState<RunResult | null>(null);

    const handleEvent = useCallback(
        (evt: PipelineEvent) => {
            setRunResult(prev => {
                const next: RunResult = prev
                    ? { ...prev, nodes: { ...prev.nodes } }
                    : {
                          status: 'ok',
                          duration_ms: 0,
                          nodes: {},
                          preview: [],
                      };
                switch (evt.type) {
                    case 'started':
                        return { status: 'ok', duration_ms: 0, nodes: {}, preview: [] };
                    case 'stage_started':
                        next.nodes[evt.node_id] = { status: 'running', kind: evt.kind };
                        break;
                    case 'stage_finished':
                        next.nodes[evt.node_id] = {
                            status: evt.status,
                            kind: evt.kind,
                            rows: evt.rows,
                            duration_ms: evt.duration_ms,
                            error: evt.error,
                        };
                        break;
                    case 'cancelled':
                        next.status = 'cancelled';
                        break;
                    case 'finished':
                        next.status = evt.status;
                        next.duration_ms = evt.duration_ms;
                        break;
                }
                return next;
            });
        },
        [],
    );

    const finishRun = useCallback(
        (start: number, result: RunResult | null) => {
            if (result) {
                setRunResult(result);
                // Merge the previews back into each node's data so the
                // Preview tab and the inline schema badge stay in sync
                // with what just ran.
                if (result.preview.length > 0) {
                    const byId = new Map(result.preview.map(p => [p.node_id, p]));
                    setNodes(ns =>
                        ns.map(n => {
                            const p = byId.get(n.id);
                            if (!p) return n;
                            return {
                                ...n,
                                data: {
                                    ...n.data,
                                    schema: p.columns,
                                    sampleRows: p.rows,
                                },
                            };
                        }),
                    );
                }
            } else {
                setRunResult({
                    status: 'error',
                    duration_ms: Math.round(performance.now() - start),
                    nodes: {},
                    preview: [],
                    error:
                        'Pipeline execution is only available in the desktop app. Launch with `cargo run -p duckle-desktop`.',
                });
            }
        },
        [setNodes],
    );

    const handleRun = useCallback(() => {
        setIsRunning(true);
        setRunResult(null);
        const start = performance.now();
        void runPipeline(nodes, edges, handleEvent)
            .then(result => finishRun(start, result))
            .finally(() => setIsRunning(false));
    }, [nodes, edges, handleEvent, finishRun]);

    const handleRunFromHere = useCallback(
        (nodeId: string) => {
            setIsRunning(true);
            setRunResult(null);
            const start = performance.now();
            void runPipelinePartial(nodes, edges, nodeId, handleEvent)
                .then(result => finishRun(start, result))
                .finally(() => setIsRunning(false));
        },
        [nodes, edges, handleEvent, finishRun],
    );

    const handleStop = useCallback(() => {
        void cancelPipeline();
    }, []);

    const nodeLabels = useMemo(() => {
        const m: Record<string, string> = {};
        for (const n of nodes) m[n.id] = n.data.label;
        return m;
    }, [nodes]);

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
                void manifest.autodetect(newNode.data.properties ?? {}).then(result => {
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
                    void manifest.autodetect(node.data.properties ?? {}).then(result => {
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
                    handleRunFromHere(nodeId);
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
        [nodes, selectedId, setNodes, setEdges, markDirty, handleRunFromHere],
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

    // Repo-item editor modal state (connections / contexts / docs / routines)
    type EditorState =
        | { kind: 'connection'; itemId: string | null; parentId: string }
        | { kind: 'context'; itemId: string | null; parentId: string }
        | { kind: 'document'; itemId: string | null; parentId: string }
        | { kind: 'routine'; itemId: string | null; parentId: string }
        | null;
    const [repoEditor, setRepoEditor] = useState<EditorState>(null);

    const handleNewConnection = useCallback(
        (parentId: string) => setRepoEditor({ kind: 'connection', itemId: null, parentId }),
        [],
    );
    const handleNewContext = useCallback(
        (parentId: string) => setRepoEditor({ kind: 'context', itemId: null, parentId }),
        [],
    );
    const handleNewDocument = useCallback(
        (parentId: string) => setRepoEditor({ kind: 'document', itemId: null, parentId }),
        [],
    );
    const handleNewRoutine = useCallback(
        (parentId: string) => setRepoEditor({ kind: 'routine', itemId: null, parentId }),
        [],
    );

    const handleOpenRepoItem = useCallback((item: RepoItem) => {
        if (item.type === 'connection')
            setRepoEditor({
                kind: 'connection',
                itemId: item.id,
                parentId: item.parentId ?? 'connections',
            });
        else if (item.type === 'context')
            setRepoEditor({
                kind: 'context',
                itemId: item.id,
                parentId: item.parentId ?? 'contexts',
            });
        else if (item.type === 'doc')
            setRepoEditor({
                kind: 'document',
                itemId: item.id,
                parentId: item.parentId ?? 'docs',
            });
        else if (item.type === 'routine')
            setRepoEditor({
                kind: 'routine',
                itemId: item.id,
                parentId: item.parentId ?? 'routines',
            });
    }, []);

    const editingRepoItem = useMemo(
        () => (repoEditor?.itemId ? repo.find(i => i.id === repoEditor.itemId) ?? null : null),
        [repoEditor, repo],
    );

    const upsertRepoItem = useCallback(
        (
            type: 'connection' | 'context' | 'doc' | 'routine',
            name: string,
            payload: unknown,
        ) => {
            if (!repoEditor) return;
            if (repoEditor.itemId) {
                setRepo(r =>
                    r.map(i =>
                        i.id === repoEditor.itemId
                            ? { ...i, name, payload: payload as RepoItem['payload'] }
                            : i,
                    ),
                );
            } else {
                const id =
                    type[0] +
                    '_' +
                    Date.now().toString(36) +
                    '_' +
                    Math.random().toString(36).slice(2, 6);
                setRepo(r => [
                    ...r,
                    {
                        id,
                        name,
                        type,
                        parentId: repoEditor.parentId,
                        payload: payload as RepoItem['payload'],
                    },
                ]);
            }
            setRepoEditor(null);
        },
        [repoEditor],
    );

    const handleSaveConnection = useCallback(
        (name: string, payload: ConnectionPayload) => upsertRepoItem('connection', name, payload),
        [upsertRepoItem],
    );
    const handleSaveContext = useCallback(
        (name: string, payload: ContextPayload) => upsertRepoItem('context', name, payload),
        [upsertRepoItem],
    );
    const handleSaveDocument = useCallback(
        (name: string, payload: DocumentPayload) => upsertRepoItem('doc', name, payload),
        [upsertRepoItem],
    );
    const handleSaveRoutine = useCallback(
        (name: string, payload: RoutinePayload) => upsertRepoItem('routine', name, payload),
        [upsertRepoItem],
    );

    const openJobIds = useMemo(() => new Set(jobs.map(j => j.id)), [jobs]);

    return (
        <RunStatusContext.Provider value={runResult?.nodes ?? {}}>
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
                <button
                    type="button"
                    className="topbar-theme-toggle"
                    onClick={toggleTheme}
                    title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
                    aria-label="Toggle theme"
                >
                    {theme === 'dark' ? <Sun size={14} /> : <Moon size={14} />}
                </button>
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
                    onOpenItem={handleOpenRepoItem}
                    onNewPipeline={openNewPipelineModal}
                    onNewFolder={handleNewFolderInRepo}
                    onNewConnection={handleNewConnection}
                    onNewContext={handleNewContext}
                    onNewDocument={handleNewDocument}
                    onNewRoutine={handleNewRoutine}
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
                        onEdgeEdit={handleEdgeEdit}
                        nodeAutodetectAvailable={nodeAutodetectAvailable}
                    />
                </section>
                <PropertiesPanel
                    selected={selectedNode}
                    allNodes={nodes}
                    edges={edges}
                    repoItems={repo}
                    onUpdate={handleUpdateNode}
                    onOpenMapper={handleOpenMapper}
                    focusNameRequest={renameRequest}
                />
            </main>

            <BottomPanel
                runResult={runResult}
                isRunning={isRunning}
                nodeLabels={nodeLabels}
            />

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

            {showWorkspacePicker ? (
                <WorkspacePickerModal onPicked={handlePickedWorkspace} />
            ) : null}

            {editingEdge ? (
                <EdgeEditorModal
                    edge={editingEdge}
                    onSave={handleEdgeEditSave}
                    onCancel={() => setEditingEdgeId(null)}
                />
            ) : null}

            {repoEditor?.kind === 'connection' ? (
                <ConnectionEditorModal
                    item={editingRepoItem}
                    onSave={handleSaveConnection}
                    onCancel={() => setRepoEditor(null)}
                />
            ) : null}
            {repoEditor?.kind === 'context' ? (
                <ContextEditorModal
                    item={editingRepoItem}
                    onSave={handleSaveContext}
                    onCancel={() => setRepoEditor(null)}
                />
            ) : null}
            {repoEditor?.kind === 'document' ? (
                <DocumentEditorModal
                    item={editingRepoItem}
                    onSave={handleSaveDocument}
                    onCancel={() => setRepoEditor(null)}
                />
            ) : null}
            {repoEditor?.kind === 'routine' ? (
                <RoutineEditorModal
                    item={editingRepoItem}
                    onSave={handleSaveRoutine}
                    onCancel={() => setRepoEditor(null)}
                />
            ) : null}

            {mapperNode ? (
                <VisualMapperModal
                    nodeId={mapperNode.id}
                    nodeLabel={mapperNode.data.label}
                    nodes={nodes}
                    edges={edges}
                    initialState={
                        ((mapperNode.data.properties?.mapper as MapperState | undefined) ?? {
                            outputs: [] as MappingRow[],
                        }) as MapperState
                    }
                    onSave={handleMapperSave}
                    onCancel={() => setMapperNodeId(null)}
                />
            ) : null}
        </div>
        </RunStatusContext.Provider>
    );
}
