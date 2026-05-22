import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { CheckCircle2, Cpu, Download, Loader2, Workflow } from 'lucide-react';
import {
    engineInstall,
    engineStatus,
    type EngineStatus,
    type InstallProgress,
} from '../tauri-bridge';

type Props = {
    onReady: () => void;
};

export default function EngineSetupModal({ onReady }: Props) {
    const [engines, setEngines] = useState<EngineStatus[]>([]);
    const [loading, setLoading] = useState(true);
    const [progress, setProgress] = useState<Record<string, InstallProgress>>({});
    const [installing, setInstalling] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        const list = await engineStatus();
        setEngines(list);
        setLoading(false);
        return list;
    }, []);

    useEffect(() => {
        void refresh();
    }, [refresh]);

    const duckdb = engines.find(e => e.id === 'duckdb');
    const canContinue = Boolean(duckdb?.installed);

    const install = async (id: string) => {
        setInstalling(id);
        setError(null);
        setProgress(p => ({ ...p, [id]: { phase: 'downloading', received: 0 } }));
        try {
            await engineInstall(id, p => {
                setProgress(prev => ({ ...prev, [id]: p }));
            });
            await refresh();
        } catch (err) {
            setError(String(err));
            setProgress(p => ({ ...p, [id]: { phase: 'failed', error: String(err) } }));
        } finally {
            setInstalling(null);
        }
    };

    return createPortal(
        <div className="modal-backdrop modal-backdrop-blocking">
            <div className="modal modal-engine">
                <div className="modal-header modal-workspace-header">
                    <div className="modal-workspace-mark">
                        <Workflow size={28} />
                    </div>
                    <div className="modal-workspace-titles">
                        <div className="modal-title">Set up Duckle</div>
                        <div className="modal-subtitle">
                            One-time engine install - Duckle stays a tiny download and fetches
                            its engine on first run.
                        </div>
                    </div>
                </div>

                <div className="modal-body modal-engine-body">
                    {loading ? (
                        <div className="engine-loading">
                            <Loader2 size={16} className="spin" /> Checking installed engines…
                        </div>
                    ) : (
                        <div className="engine-list">
                            {engines.map(e => (
                                <EngineRow
                                    key={e.id}
                                    engine={e}
                                    progress={progress[e.id]}
                                    installing={installing === e.id}
                                    disabled={installing !== null}
                                    onInstall={() => install(e.id)}
                                />
                            ))}
                        </div>
                    )}
                    {error ? <div className="modal-engine-error">{error}</div> : null}
                </div>

                <div className="modal-footer modal-engine-footer">
                    <span className="modal-engine-hint">
                        {canContinue
                            ? 'DuckDB ready.'
                            : 'DuckDB is required to run pipelines.'}
                    </span>
                    <button
                        type="button"
                        className="btn btn-primary"
                        onClick={onReady}
                        disabled={!canContinue}
                    >
                        Continue
                    </button>
                </div>
            </div>
        </div>,
        document.body,
    );
}

function EngineRow({
    engine,
    progress,
    installing,
    disabled,
    onInstall,
}: {
    engine: EngineStatus;
    progress?: InstallProgress;
    installing: boolean;
    disabled: boolean;
    onInstall: () => void;
}) {
    return (
        <div className="engine-row">
            <div className="engine-row-icon">
                <Cpu size={16} />
            </div>
            <div className="engine-row-info">
                <div className="engine-row-head">
                    <span className="engine-row-name">{engine.name}</span>
                    {engine.required ? (
                        <span className="engine-row-tag required">required</span>
                    ) : (
                        <span className="engine-row-tag">optional</span>
                    )}
                    {engine.version ? (
                        <span className="engine-row-ver">v{engine.version}</span>
                    ) : null}
                </div>
                <div className="engine-row-desc">{engine.description}</div>
                {installing && progress ? (
                    <ProgressLine progress={progress} />
                ) : null}
            </div>
            <div className="engine-row-action">
                {engine.installed ? (
                    <span className="engine-row-installed">
                        <CheckCircle2 size={14} /> Installed
                    </span>
                ) : installing ? (
                    <span className="engine-row-installing">
                        <Loader2 size={13} className="spin" /> Installing…
                    </span>
                ) : engine.available ? (
                    <button
                        type="button"
                        className="btn btn-primary btn-sm"
                        onClick={onInstall}
                        disabled={disabled}
                    >
                        <Download size={13} /> Install
                    </button>
                ) : (
                    <span className="engine-row-unavailable">Not available</span>
                )}
            </div>
        </div>
    );
}

function ProgressLine({ progress }: { progress: InstallProgress }) {
    let label = '';
    let pct: number | null = null;
    switch (progress.phase) {
        case 'downloading': {
            const mb = (progress.received / 1_000_000).toFixed(1);
            if (progress.total) {
                pct = Math.round((progress.received / progress.total) * 100);
                const totalMb = (progress.total / 1_000_000).toFixed(1);
                label = `Downloading ${mb} / ${totalMb} MB`;
            } else {
                label = `Downloading ${mb} MB`;
            }
            break;
        }
        case 'extracting':
            label = 'Extracting…';
            break;
        case 'verifying':
            label = 'Verifying…';
            break;
        case 'done':
            label = 'Done';
            pct = 100;
            break;
        case 'failed':
            label = progress.error;
            break;
    }
    return (
        <div className="engine-progress">
            <div className="engine-progress-bar">
                <div
                    className="engine-progress-fill"
                    style={{ width: pct != null ? `${pct}%` : '40%' }}
                    data-indeterminate={pct == null}
                />
            </div>
            <div className="engine-progress-label">{label}</div>
        </div>
    );
}
