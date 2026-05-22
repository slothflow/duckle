import { useCallback, useEffect, useRef, useState } from 'react';
import {
    AlertCircle,
    CheckCircle2,
    ChevronDown,
    ChevronUp,
    PlayCircle,
    Terminal,
} from 'lucide-react';
import type { RunResult } from '../tauri-bridge';
import type { ValidationResult } from '../validation';
import { friendlyError } from '../errors';

type TabId = 'problems' | 'output' | 'console';

const MIN_HEIGHT = 100;
const MAX_HEIGHT = 600;
const DEFAULT_HEIGHT = 260;

export type Props = {
    runResult: RunResult | null;
    isRunning: boolean;
    nodeLabels: Record<string, string>;
    terminalNodeIds: string[];
    validation: ValidationResult;
    openProblemsRequest?: number;
};

export default function BottomPanel({
    runResult,
    isRunning,
    nodeLabels,
    terminalNodeIds,
    validation,
    openProblemsRequest,
}: Props) {
    const [tab, setTab] = useState<TabId>('problems');
    const [collapsed, setCollapsed] = useState<boolean>(true);
    const [height, setHeight] = useState<number>(DEFAULT_HEIGHT);
    const dragRef = useRef<{ startY: number; startH: number } | null>(null);

    // Auto-expand Output tab when a run finishes.
    useEffect(() => {
        if (runResult) {
            setTab('output');
            setCollapsed(false);
        }
    }, [runResult]);

    // Auto-expand Problems tab when Validate is clicked.
    useEffect(() => {
        if (openProblemsRequest && openProblemsRequest > 0) {
            setTab('problems');
            setCollapsed(false);
        }
    }, [openProblemsRequest]);

    const onResizeStart = useCallback(
        (e: React.MouseEvent) => {
            if (collapsed) return;
            dragRef.current = { startY: e.clientY, startH: height };
            e.preventDefault();
        },
        [collapsed, height],
    );

    useEffect(() => {
        const onMove = (e: MouseEvent) => {
            if (!dragRef.current) return;
            const dy = dragRef.current.startY - e.clientY;
            const next = Math.max(MIN_HEIGHT, Math.min(MAX_HEIGHT, dragRef.current.startH + dy));
            setHeight(next);
        };
        const onUp = () => {
            dragRef.current = null;
        };
        document.addEventListener('mousemove', onMove);
        document.addEventListener('mouseup', onUp);
        return () => {
            document.removeEventListener('mousemove', onMove);
            document.removeEventListener('mouseup', onUp);
        };
    }, []);

    const onTabClick = (id: TabId) => {
        if (collapsed) {
            setCollapsed(false);
            setTab(id);
        } else if (tab === id) {
            setCollapsed(true);
        } else {
            setTab(id);
        }
    };

    const runErrors = runResult
        ? Object.entries(runResult.nodes).filter(([, st]) => st.status === 'error')
        : [];

    const problemsBadge = validation.errorCount + validation.warningCount + runErrors.length;

    const tabs: { id: TabId; label: string; badge?: number }[] = [
        { id: 'problems', label: 'Problems', badge: problemsBadge },
        { id: 'output', label: 'Output' },
        { id: 'console', label: 'Console' },
    ];

    return (
        <div
            className={'bottom-panel' + (collapsed ? ' is-collapsed' : '')}
            style={collapsed ? undefined : { height }}
        >
            <div className="bottom-panel-resize" onMouseDown={onResizeStart} aria-hidden="true" />
            <div className="bottom-panel-tabs" role="tablist">
                {tabs.map(t => (
                    <button
                        key={t.id}
                        type="button"
                        role="tab"
                        aria-selected={!collapsed && tab === t.id}
                        className="bottom-panel-tab"
                        onClick={() => onTabClick(t.id)}
                    >
                        {t.label}
                        {t.badge !== undefined && t.badge > 0 ? (
                            <span className="bottom-panel-tab-badge">{t.badge}</span>
                        ) : null}
                    </button>
                ))}
                <div className="bottom-panel-spacer" />
                <button
                    type="button"
                    className="bottom-panel-toggle"
                    onClick={() => setCollapsed(c => !c)}
                    aria-label={collapsed ? 'Expand panel' : 'Collapse panel'}
                >
                    {collapsed ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                </button>
            </div>
            {!collapsed ? (
                <div className="bottom-panel-content">
                    {tab === 'problems' ? (
                        <ProblemsTab
                            validation={validation}
                            runErrors={runErrors}
                            nodeLabels={nodeLabels}
                        />
                    ) : null}
                    {tab === 'output' ? (
                        <OutputTab
                            runResult={runResult}
                            isRunning={isRunning}
                            nodeLabels={nodeLabels}
                            terminalNodeIds={terminalNodeIds}
                        />
                    ) : null}
                    {tab === 'console' ? (
                        <ConsoleTab runResult={runResult} nodeLabels={nodeLabels} />
                    ) : null}
                </div>
            ) : null}
        </div>
    );
}

function ProblemsTab({
    validation,
    runErrors,
    nodeLabels,
}: {
    validation: ValidationResult;
    runErrors: [string, { error?: string }][];
    nodeLabels: Record<string, string>;
}) {
    const hasNothing = validation.issues.length === 0 && runErrors.length === 0;
    if (hasNothing) {
        return (
            <div className="bottom-empty">
                <CheckCircle2 size={22} className="bottom-empty-icon bottom-empty-icon-ok" />
                <div className="bottom-empty-title">No problems detected</div>
                <div className="bottom-empty-desc">
                    Required properties, dangling edges, cycles, and execution errors surface
                    here. Validation runs live as you edit the pipeline.
                </div>
            </div>
        );
    }
    return (
        <div className="bottom-problems">
            {validation.issues.map(issue => (
                <ProblemRow
                    key={issue.id}
                    severity={issue.severity}
                    title={
                        issue.nodeId
                            ? nodeLabels[issue.nodeId] ?? issue.nodeId
                            : 'Pipeline'
                    }
                    detail={issue.message}
                    code={issue.code}
                />
            ))}
            {runErrors.map(([nodeId, st]) => (
                <ProblemRow
                    key={'r_' + nodeId}
                    severity="error"
                    title={nodeLabels[nodeId] ?? nodeId}
                    detail={friendlyError(st.error) || 'Execution failed.'}
                    code="run-error"
                />
            ))}
        </div>
    );
}

function ProblemRow({
    severity,
    title,
    detail,
    code,
}: {
    severity: 'error' | 'warning';
    title: string;
    detail: string;
    code: string;
}) {
    return (
        <div className={'bottom-problem-row severity-' + severity}>
            <AlertCircle size={13} className="bottom-problem-icon" />
            <div className="bottom-problem-body">
                <div className="bottom-problem-title">{title}</div>
                <div className="bottom-problem-detail">{detail}</div>
            </div>
            <code className="bottom-problem-code">{code}</code>
        </div>
    );
}

function OutputTab({
    runResult,
    isRunning,
    nodeLabels,
    terminalNodeIds,
}: {
    runResult: RunResult | null;
    isRunning: boolean;
    nodeLabels: Record<string, string>;
    terminalNodeIds: string[];
}) {
    if (isRunning) {
        return (
            <div className="bottom-empty">
                <PlayCircle size={22} className="bottom-empty-icon bottom-empty-icon-ok" />
                <div className="bottom-empty-title">Running…</div>
                <div className="bottom-empty-desc">
                    Executing the pipeline through the DuckDB engine.
                </div>
            </div>
        );
    }
    if (!runResult) {
        return (
            <div className="bottom-empty">
                <div className="bottom-empty-title">No run output yet</div>
                <div className="bottom-empty-desc">
                    Press <kbd className="kbd">F5</kbd> or click <b>Run</b> to execute the
                    pipeline. Logs, per-node row counts, and execution timings stream here.
                </div>
            </div>
        );
    }

    const totals = runStats(runResult);
    // Show only the pipeline's terminal results, not a stacked table per
    // intermediate stage. Fall back to all previews if we can't tell.
    const terminalSet = new Set(terminalNodeIds);
    const terminalPreviews =
        terminalNodeIds.length > 0
            ? runResult.preview.filter(p => terminalSet.has(p.node_id))
            : runResult.preview;
    return (
        <div className="bottom-output">
            <div className="bottom-output-summary">
                <span className={'bottom-status status-' + runResult.status}>
                    {runResult.status === 'ok' ? <CheckCircle2 size={12} /> : <AlertCircle size={12} />}
                    {runResult.status === 'ok' ? 'Run succeeded' : 'Run failed'}
                </span>
                <span className="bottom-output-stat">
                    <b>{totals.nodeCount}</b> nodes
                </span>
                <span className="bottom-output-stat">
                    <b>{totals.rowsWritten.toLocaleString()}</b> rows written
                </span>
                <span className="bottom-output-stat">
                    <b>{runResult.duration_ms} ms</b> total
                </span>
            </div>
            <div className="bottom-output-rows">
                {Object.entries(runResult.nodes).map(([nodeId, st]) => (
                    <div className={'bottom-output-row status-' + st.status} key={nodeId}>
                        <span className="bottom-output-dot" />
                        <span className="bottom-output-label">
                            {nodeLabels[nodeId] ?? nodeId}
                        </span>
                        <span className="bottom-output-kind">{st.kind ?? ''}</span>
                        {st.rows !== undefined ? (
                            <span className="bottom-output-rows-stat">
                                {st.rows.toLocaleString()} rows
                            </span>
                        ) : (
                            <span className="bottom-output-rows-stat" />
                        )}
                        <span className="bottom-output-time">
                            {st.duration_ms !== undefined ? st.duration_ms + ' ms' : ''}
                        </span>
                        {st.error ? (
                            <span className="bottom-output-error">
                                {friendlyError(st.error)}
                            </span>
                        ) : null}
                    </div>
                ))}
            </div>
            {runResult.error ? (
                <div className="bottom-output-error-banner">
                    {friendlyError(runResult.error)}
                </div>
            ) : null}
            {terminalPreviews.length > 0 ? (
                <div className="bottom-output-previews">
                    {terminalPreviews.map(p => (
                        <PreviewTable key={p.node_id} preview={p} label={nodeLabels[p.node_id]} />
                    ))}
                </div>
            ) : null}
        </div>
    );
}

function PreviewTable({
    preview,
    label,
}: {
    preview: { node_id: string; columns: { name: string; type: string }[]; rows: Record<string, unknown>[] };
    label?: string;
}) {
    return (
        <div className="bottom-preview">
            <div className="bottom-preview-head">
                Preview · <b>{label ?? preview.node_id}</b> · {preview.rows.length} rows
            </div>
            <div className="bottom-preview-scroll">
                <table className="bottom-preview-table">
                    <thead>
                        <tr>
                            {preview.columns.map(c => (
                                <th key={c.name}>
                                    {c.name}
                                    <span className="bottom-preview-coltype">{c.type}</span>
                                </th>
                            ))}
                        </tr>
                    </thead>
                    <tbody>
                        {preview.rows.map((r, i) => (
                            <tr key={i}>
                                {preview.columns.map(c => (
                                    <td key={c.name}>
                                        {formatCell(r[c.name])}
                                    </td>
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
    if (v === null || v === undefined) return '';
    if (typeof v === 'object') return JSON.stringify(v);
    return String(v);
}

function ConsoleTab({
    runResult,
    nodeLabels,
}: {
    runResult: RunResult | null;
    nodeLabels: Record<string, string>;
}) {
    if (!runResult) {
        return (
            <div className="bottom-empty bottom-console">
                <div className="bottom-console-line">
                    <Terminal size={12} className="bottom-console-icon" />
                    <span className="bottom-console-time">[ready]</span>
                    <span className="bottom-console-msg">
                        The execution log. Run a pipeline (▶ Run) and each stage's status,
                        row count, and timing - plus any errors - appears here.
                    </span>
                </div>
            </div>
        );
    }
    const lines = Object.entries(runResult.nodes).map(([id, st]) => {
        const label = nodeLabels[id] ?? id;
        const tag = st.status === 'ok' ? 'ok' : st.status;
        const detail =
            st.status === 'ok'
                ? `${label} - ${st.kind ?? 'stage'} - ${st.rows ?? 0} rows - ${st.duration_ms ?? 0} ms`
                : `${label} - ${friendlyError(st.error) || st.status}`;
        return { id, tag, detail, ok: st.status === 'ok' };
    });
    return (
        <div className="bottom-console">
            <div className="bottom-console-line">
                <Terminal size={12} className="bottom-console-icon" />
                <span className="bottom-console-time">[run]</span>
                <span className="bottom-console-msg">
                    {runResult.status === 'ok' ? 'Pipeline finished' : 'Pipeline ' + runResult.status} ·{' '}
                    {runResult.duration_ms} ms
                </span>
            </div>
            {lines.map(l => (
                <div className="bottom-console-line" key={l.id}>
                    <span
                        className={
                            'bottom-console-tag ' +
                            (l.ok ? 'bottom-console-tag-ok' : 'bottom-console-tag-err')
                        }
                    >
                        [{l.tag}]
                    </span>
                    <span className="bottom-console-msg">{l.detail}</span>
                </div>
            ))}
        </div>
    );
}

function runStats(r: RunResult) {
    let rowsWritten = 0;
    let nodeCount = 0;
    for (const st of Object.values(r.nodes)) {
        nodeCount += 1;
        if (st.rows) rowsWritten += st.rows;
    }
    return { rowsWritten, nodeCount };
}
