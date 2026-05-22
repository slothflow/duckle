import { useCallback, useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import {
    AlarmClock,
    CalendarClock,
    Eye,
    FolderOpen,
    Pencil,
    Play,
    Plus,
    Repeat,
    Trash2,
    X,
} from 'lucide-react';
import { isTauri } from '../tauri-dialog';
import {
    runHistory,
    scheduleDelete,
    scheduleList,
    scheduleRunNow,
    scheduleUpsert,
    type RunRecord,
    type Schedule,
    type ScheduleKind,
} from '../tauri-bridge';

type Props = {
    pipelineId: string;
    pipelineName: string;
    workspacePath: string | null;
    onClose: () => void;
};

type Draft = {
    id: string;
    name: string;
    enabled: boolean;
    mode: 'cron' | 'interval' | 'file_watch';
    cronExpr: string;
    intervalUnit: 'seconds' | 'minutes' | 'hours' | 'days';
    intervalValue: number;
    watchPath: string;
    watchRecursive: boolean;
};

const CRON_PRESETS: { label: string; expr: string }[] = [
    { label: 'Every minute', expr: '0 * * * * *' },
    { label: 'Every 5 minutes', expr: '0 */5 * * * *' },
    { label: 'Every hour', expr: '0 0 * * * *' },
    { label: 'Every day at 03:00', expr: '0 0 3 * * *' },
    { label: 'Mon-Fri at 08:00', expr: '0 0 8 * * Mon-Fri' },
];

export default function ScheduleEditorModal({
    pipelineId,
    pipelineName,
    workspacePath,
    onClose,
}: Props) {
    const [schedules, setSchedules] = useState<Schedule[]>([]);
    const [history, setHistory] = useState<RunRecord[]>([]);
    const [editing, setEditing] = useState<Draft | null>(null);
    const [loading, setLoading] = useState(true);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        setLoading(true);
        const all = await scheduleList();
        setSchedules(all.filter(s => s.pipeline_id === pipelineId));
        if (workspacePath) {
            setHistory(await runHistory(workspacePath, pipelineId));
        }
        setLoading(false);
    }, [pipelineId, workspacePath]);

    useEffect(() => {
        void refresh();
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape' && !editing) onClose();
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [refresh, onClose, editing]);

    const startNew = () => {
        setEditing({
            id: '',
            name: 'New schedule',
            enabled: true,
            mode: 'interval',
            cronExpr: '0 0 * * * *',
            intervalUnit: 'hours',
            intervalValue: 1,
            watchPath: '',
            watchRecursive: true,
        });
        setError(null);
    };

    const startEdit = (s: Schedule) => {
        const intervalSec = s.kind.type === 'interval' ? s.kind.seconds : 3600;
        const cronExpr = s.kind.type === 'cron' ? s.kind.expr : '0 0 * * * *';
        const { value, unit } = splitInterval(intervalSec);
        setEditing({
            id: s.id,
            name: s.name,
            enabled: s.enabled,
            mode: s.kind.type,
            cronExpr,
            intervalUnit: unit,
            intervalValue: value,
            watchPath: s.kind.type === 'file_watch' ? s.kind.path : '',
            watchRecursive: s.kind.type === 'file_watch' ? s.kind.recursive : true,
        });
        setError(null);
    };

    const saveDraft = async () => {
        if (!editing) return;
        setBusy(true);
        setError(null);
        const kind: ScheduleKind =
            editing.mode === 'cron'
                ? { type: 'cron', expr: editing.cronExpr.trim() }
                : editing.mode === 'file_watch'
                  ? {
                        type: 'file_watch',
                        path: editing.watchPath.trim(),
                        recursive: editing.watchRecursive,
                    }
                  : {
                        type: 'interval',
                        seconds: joinInterval(editing.intervalValue, editing.intervalUnit),
                    };
        const draft: Schedule = {
            id: editing.id,
            pipeline_id: pipelineId,
            name: editing.name.trim() || 'Schedule',
            enabled: editing.enabled,
            kind,
        };
        try {
            await scheduleUpsert(draft);
            setEditing(null);
            await refresh();
        } catch (err) {
            setError(String(err));
        } finally {
            setBusy(false);
        }
    };

    const handleDelete = async (id: string) => {
        setBusy(true);
        try {
            await scheduleDelete(id);
            await refresh();
        } finally {
            setBusy(false);
        }
    };

    const handleRunNow = async (id: string) => {
        setBusy(true);
        try {
            await scheduleRunNow(id);
            await refresh();
        } finally {
            setBusy(false);
        }
    };

    const handleBackdrop = (e: React.MouseEvent) => {
        if (e.target === e.currentTarget && !editing) onClose();
    };

    return createPortal(
        <div className="modal-backdrop" onClick={handleBackdrop}>
            <div className="modal modal-schedule">
                <div className="modal-header">
                    <div className="modal-title-row">
                        <AlarmClock size={16} className="modal-title-icon" />
                        <div>
                            <div className="modal-title">Schedules</div>
                            <div className="modal-subtitle">
                                Pipeline: <b>{pipelineName}</b>
                            </div>
                        </div>
                    </div>
                    <button
                        type="button"
                        className="modal-close"
                        onClick={onClose}
                        aria-label="Close"
                    >
                        <X size={16} />
                    </button>
                </div>

                <div className="modal-body modal-schedule-body">
                    {editing ? (
                        <ScheduleForm
                            draft={editing}
                            onChange={setEditing}
                            onSave={saveDraft}
                            onCancel={() => setEditing(null)}
                            busy={busy}
                            error={error}
                        />
                    ) : (
                        <>
                            <div className="schedule-list">
                                {loading ? (
                                    <div className="schedule-empty">Loading…</div>
                                ) : schedules.length === 0 ? (
                                    <div className="schedule-empty">
                                        No schedules yet. Click <b>+ New schedule</b> to set one
                                        up.
                                    </div>
                                ) : (
                                    schedules.map(s => (
                                        <ScheduleRow
                                            key={s.id}
                                            schedule={s}
                                            onEdit={() => startEdit(s)}
                                            onDelete={() => handleDelete(s.id)}
                                            onRunNow={() => handleRunNow(s.id)}
                                            busy={busy}
                                        />
                                    ))
                                )}
                            </div>
                            <button
                                type="button"
                                className="btn btn-primary schedule-add"
                                onClick={startNew}
                                disabled={busy}
                            >
                                <Plus size={13} /> New schedule
                            </button>

                            {history.length > 0 ? (
                                <div className="run-history">
                                    <div className="run-history-title">Recent runs</div>
                                    <div className="run-history-list">
                                        {history.slice(0, 12).map((r, i) => (
                                            <div
                                                className={'run-history-row status-' + r.status}
                                                key={i}
                                            >
                                                <span className="run-history-dot" />
                                                <span className="run-history-trigger">
                                                    {r.trigger}
                                                </span>
                                                <span className="run-history-time">
                                                    {formatTime(r.at)}
                                                </span>
                                                <span className="run-history-rows">
                                                    {r.rows.toLocaleString()} rows
                                                </span>
                                                <span className="run-history-dur">
                                                    {r.duration_ms} ms
                                                </span>
                                                {r.error ? (
                                                    <span
                                                        className="run-history-err"
                                                        title={r.error}
                                                    >
                                                        failed
                                                    </span>
                                                ) : null}
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            ) : null}
                        </>
                    )}
                </div>
            </div>
        </div>,
        document.body,
    );
}

function ScheduleRow({
    schedule,
    onEdit,
    onDelete,
    onRunNow,
    busy,
}: {
    schedule: Schedule;
    onEdit: () => void;
    onDelete: () => void;
    onRunNow: () => void;
    busy: boolean;
}) {
    const summary = useMemo(() => describeKind(schedule.kind), [schedule.kind]);
    return (
        <div className={'schedule-row' + (schedule.enabled ? '' : ' is-disabled')}>
            <div className="schedule-row-info">
                <div className="schedule-row-header">
                    <span className="schedule-row-name">{schedule.name}</span>
                    {schedule.enabled ? (
                        <span className="schedule-row-pill enabled">enabled</span>
                    ) : (
                        <span className="schedule-row-pill">paused</span>
                    )}
                </div>
                <div className="schedule-row-detail">
                    {schedule.kind.type === 'cron' ? (
                        <CalendarClock size={11} />
                    ) : schedule.kind.type === 'file_watch' ? (
                        <Eye size={11} />
                    ) : (
                        <Repeat size={11} />
                    )}
                    {summary}
                </div>
                <div className="schedule-row-meta">
                    {schedule.next_run_at ? (
                        <span>
                            Next: <b>{formatTime(schedule.next_run_at)}</b>
                        </span>
                    ) : null}
                    {schedule.last_run_at ? (
                        <span>
                            Last: <b>{formatTime(schedule.last_run_at)}</b>
                            {schedule.last_run_status
                                ? ' · ' + schedule.last_run_status
                                : ''}
                        </span>
                    ) : null}
                </div>
                {schedule.last_run_error ? (
                    <div className="schedule-row-error">{schedule.last_run_error}</div>
                ) : null}
            </div>
            <div className="schedule-row-actions">
                <button
                    type="button"
                    className="btn btn-icon"
                    onClick={onRunNow}
                    disabled={busy}
                    title="Run now"
                >
                    <Play size={12} />
                </button>
                <button
                    type="button"
                    className="btn btn-icon"
                    onClick={onEdit}
                    disabled={busy}
                    title="Edit"
                >
                    <Pencil size={12} />
                </button>
                <button
                    type="button"
                    className="btn btn-icon btn-icon-danger"
                    onClick={onDelete}
                    disabled={busy}
                    title="Delete"
                >
                    <Trash2 size={12} />
                </button>
            </div>
        </div>
    );
}

function ScheduleForm({
    draft,
    onChange,
    onSave,
    onCancel,
    busy,
    error,
}: {
    draft: Draft;
    onChange: (d: Draft) => void;
    onSave: () => void;
    onCancel: () => void;
    busy: boolean;
    error: string | null;
}) {
    return (
        <div className="schedule-form">
            <div className="modal-field">
                <label className="modal-field-label">Name</label>
                <input
                    type="text"
                    className="modal-input"
                    value={draft.name}
                    onChange={e => onChange({ ...draft, name: e.target.value })}
                />
            </div>
            <div className="modal-field">
                <label className="modal-field-label">Trigger</label>
                <div className="schedule-mode-toggle">
                    <button
                        type="button"
                        className={
                            'schedule-mode-button' +
                            (draft.mode === 'interval' ? ' is-active' : '')
                        }
                        onClick={() => onChange({ ...draft, mode: 'interval' })}
                    >
                        <Repeat size={12} /> Every N
                    </button>
                    <button
                        type="button"
                        className={
                            'schedule-mode-button' +
                            (draft.mode === 'cron' ? ' is-active' : '')
                        }
                        onClick={() => onChange({ ...draft, mode: 'cron' })}
                    >
                        <CalendarClock size={12} /> Cron
                    </button>
                    <button
                        type="button"
                        className={
                            'schedule-mode-button' +
                            (draft.mode === 'file_watch' ? ' is-active' : '')
                        }
                        onClick={() => onChange({ ...draft, mode: 'file_watch' })}
                    >
                        <Eye size={12} /> File watch
                    </button>
                </div>
            </div>
            {draft.mode === 'file_watch' ? (
                <>
                    <div className="modal-field">
                        <label className="modal-field-label">Watch path</label>
                        <div className="schedule-watch-row">
                            <input
                                type="text"
                                className="modal-input"
                                value={draft.watchPath}
                                onChange={e => onChange({ ...draft, watchPath: e.target.value })}
                                placeholder="C:\\data\\incoming"
                                spellCheck={false}
                            />
                            <button
                                type="button"
                                className="btn btn-secondary"
                                onClick={async () => {
                                    const picked = await pickWatchPath();
                                    if (picked) onChange({ ...draft, watchPath: picked });
                                }}
                            >
                                <FolderOpen size={13} /> Browse
                            </button>
                        </div>
                    </div>
                    <div className="modal-field">
                        <label className="schedule-toggle">
                            <input
                                type="checkbox"
                                checked={draft.watchRecursive}
                                onChange={e =>
                                    onChange({ ...draft, watchRecursive: e.target.checked })
                                }
                            />
                            Watch subfolders recursively
                        </label>
                    </div>
                    <div className="modal-tip">
                        <span>
                            The pipeline runs ~2s after any change under the watched path
                            (create / modify / delete), debounced so a burst of writes triggers
                            one run.
                        </span>
                    </div>
                </>
            ) : draft.mode === 'interval' ? (
                <div className="modal-field">
                    <label className="modal-field-label">Interval</label>
                    <div className="schedule-interval-row">
                        <span>Every</span>
                        <input
                            type="number"
                            className="modal-input schedule-interval-value"
                            min={1}
                            value={draft.intervalValue}
                            onChange={e =>
                                onChange({
                                    ...draft,
                                    intervalValue: Math.max(1, Number(e.target.value)),
                                })
                            }
                        />
                        <select
                            className="modal-input modal-select"
                            value={draft.intervalUnit}
                            onChange={e =>
                                onChange({
                                    ...draft,
                                    intervalUnit: e.target.value as Draft['intervalUnit'],
                                })
                            }
                        >
                            <option value="seconds">seconds</option>
                            <option value="minutes">minutes</option>
                            <option value="hours">hours</option>
                            <option value="days">days</option>
                        </select>
                    </div>
                </div>
            ) : (
                <>
                    <div className="modal-field">
                        <label className="modal-field-label">Cron expression</label>
                        <input
                            type="text"
                            className="modal-input"
                            value={draft.cronExpr}
                            onChange={e => onChange({ ...draft, cronExpr: e.target.value })}
                            spellCheck={false}
                            placeholder="0 */5 * * * *"
                        />
                    </div>
                    <div className="modal-field">
                        <label className="modal-field-label">Presets</label>
                        <div className="schedule-presets">
                            {CRON_PRESETS.map(p => (
                                <button
                                    key={p.expr}
                                    type="button"
                                    className="schedule-preset"
                                    onClick={() => onChange({ ...draft, cronExpr: p.expr })}
                                >
                                    {p.label}
                                </button>
                            ))}
                        </div>
                    </div>
                </>
            )}
            <div className="modal-field">
                <label className="schedule-toggle">
                    <input
                        type="checkbox"
                        checked={draft.enabled}
                        onChange={e => onChange({ ...draft, enabled: e.target.checked })}
                    />
                    Enabled
                </label>
            </div>
            {error ? <div className="modal-error">{error}</div> : null}
            <div className="modal-footer">
                <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={onCancel}
                    disabled={busy}
                >
                    Cancel
                </button>
                <button
                    type="button"
                    className="btn btn-primary"
                    onClick={onSave}
                    disabled={busy}
                >
                    {busy ? 'Saving…' : 'Save schedule'}
                </button>
            </div>
        </div>
    );
}

function splitInterval(seconds: number): {
    value: number;
    unit: 'seconds' | 'minutes' | 'hours' | 'days';
} {
    if (seconds % 86400 === 0) return { value: seconds / 86400, unit: 'days' };
    if (seconds % 3600 === 0) return { value: seconds / 3600, unit: 'hours' };
    if (seconds % 60 === 0) return { value: seconds / 60, unit: 'minutes' };
    return { value: seconds, unit: 'seconds' };
}

function joinInterval(value: number, unit: 'seconds' | 'minutes' | 'hours' | 'days'): number {
    switch (unit) {
        case 'days':
            return value * 86400;
        case 'hours':
            return value * 3600;
        case 'minutes':
            return value * 60;
        default:
            return value;
    }
}

function describeKind(kind: ScheduleKind): string {
    if (kind.type === 'cron') return `Cron: ${kind.expr}`;
    if (kind.type === 'file_watch') return `Watch: ${kind.path}`;
    const { value, unit } = splitInterval(kind.seconds);
    return `Every ${value} ${unit}`;
}

async function pickWatchPath(): Promise<string | null> {
    if (!isTauri()) return null;
    try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const picked = await open({
            directory: true,
            multiple: false,
            title: 'Choose a folder to watch',
        });
        return typeof picked === 'string' ? picked : null;
    } catch {
        return null;
    }
}

function formatTime(iso: string): string {
    try {
        const d = new Date(iso);
        const now = Date.now();
        const delta = d.getTime() - now;
        const abs = Math.abs(delta);
        if (abs < 60_000) return delta > 0 ? 'in <1 min' : 'just now';
        if (abs < 3_600_000) {
            const m = Math.round(abs / 60_000);
            return delta > 0 ? `in ${m} min` : `${m} min ago`;
        }
        if (abs < 86_400_000) {
            const h = Math.round(abs / 3_600_000);
            return delta > 0 ? `in ${h} h` : `${h} h ago`;
        }
        return d.toLocaleString();
    } catch {
        return iso;
    }
}
