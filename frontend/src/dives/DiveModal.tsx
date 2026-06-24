// Create, edit, or view a dive. View mode runs the saved query live (DivePanel);
// edit/create mode is a title + SQL editor with a live Preview and Save. v1 dives
// are self-contained SQL (read their source inline). See docs/design/dives.md.

import { useState } from 'react';
import type { RepoItem } from '../repo-types';
import { loadDive } from './dive-io';
import { DivePanel } from './DivePanel';
import { runDive } from './dive-run';
import { buildDiveHtml } from './dive-export';
import { generateDive } from './dive-generate';
import { saveTextFile } from '../tauri-io';
import { DIVE_SCHEMA_VERSION, type Dive } from './dive-types';

interface DiveModalProps {
    item: RepoItem | null;
    workspacePath: string | null;
    theme?: 'light' | 'dark';
    onClose: () => void;
    onSave: (name: string, dive: Dive) => void;
}

function newId(): string {
    return 'dive_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 6);
}

export function DiveModal({ item, workspacePath, theme, onClose, onSave }: DiveModalProps) {
    let loadError: string | null = null;
    let existing: Dive | null = null;
    try {
        if (item?.payload) existing = loadDive(item.payload);
    } catch (e) {
        loadError = e instanceof Error ? e.message : String(e);
    }

    const isCreate = !existing && !loadError;
    const [editing, setEditing] = useState(isCreate);
    const [title, setTitle] = useState(existing?.title ?? item?.name ?? 'New dive');
    const [sql, setSql] = useState(existing?.query.sql ?? '');
    const [preview, setPreview] = useState<Dive | null>(existing ?? null);
    const [genChart, setGenChart] = useState<Dive['chart'] | null>(null);

    const draft = (): Dive => ({
        diveSchemaVersion: DIVE_SCHEMA_VERSION,
        id: existing?.id ?? newId(),
        title: title.trim() || 'Untitled dive',
        query: { sql },
        chart: genChart ?? existing?.chart ?? {},
        meta: { ...(existing?.meta ?? {}), generator: genChart ? 'duckie' : 'manual' },
    });

    const canRun = sql.trim().length > 0;
    const headTitle = editing ? (isCreate ? 'New dive' : 'Edit dive') : item?.name ?? 'Dive';

    const [exporting, setExporting] = useState(false);
    const exportHtml = async () => {
        const d = existing ?? (canRun ? draft() : null);
        if (!d) return;
        setExporting(true);
        try {
            const res = await runDive(d, workspacePath);
            const html = buildDiveHtml(d, res.columns, res.rows);
            const slug = (d.title || 'dive').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'dive';
            await saveTextFile(`${slug}.html`, html, [{ name: 'HTML', extensions: ['html'] }]);
        } catch (e) {
            console.error('dive export failed', e);
        } finally {
            setExporting(false);
        }
    };

    const [fromExpr, setFromExpr] = useState('');
    const [question, setQuestion] = useState('');
    const [generating, setGenerating] = useState(false);
    const [genError, setGenError] = useState<string | null>(null);
    const generate = async () => {
        if (!fromExpr.trim() || !question.trim()) return;
        setGenerating(true);
        setGenError(null);
        try {
            const gen = await generateDive(fromExpr.trim(), question.trim(), workspacePath);
            setSql(gen.sql);
            setGenChart(gen.chart);
            const t = title.trim() && title !== 'New dive' ? title.trim() : question.trim().slice(0, 60);
            setTitle(t);
            setPreview({
                diveSchemaVersion: DIVE_SCHEMA_VERSION,
                id: existing?.id ?? newId(),
                title: t || 'Untitled dive',
                query: { sql: gen.sql },
                chart: gen.chart,
                meta: { generator: 'duckie' },
            });
        } catch (e) {
            setGenError(e instanceof Error ? e.message : String(e));
        } finally {
            setGenerating(false);
        }
    };

    return (
        <div className="dive-modal-backdrop" onClick={onClose}>
            <div className="dive-modal" onClick={(e) => e.stopPropagation()}>
                <div className="dive-modal-head">
                    <span>{headTitle}</span>
                    <div className="dive-modal-actions">
                        <button
                            className="dive-btn"
                            onClick={() => void exportHtml()}
                            disabled={exporting || (!existing && !canRun)}
                            title="Export a self-contained HTML snapshot"
                        >
                            {exporting ? 'Exporting…' : 'Export HTML'}
                        </button>
                        {!editing ? (
                            <button className="dive-btn" onClick={() => setEditing(true)}>
                                Edit
                            </button>
                        ) : null}
                        <button className="dive-modal-x" onClick={onClose} aria-label="Close" title="Close">
                            ×
                        </button>
                    </div>
                </div>
                <div className="dive-modal-body">
                    {loadError ? (
                        <div className="dive-panel-msg dive-panel-err">{loadError}</div>
                    ) : editing ? (
                        <div className="dive-editor">
                            <div className="dive-gen">
                                <div className="dive-gen-title">Generate with Duckie (optional)</div>
                                <label className="dive-field">
                                    <span>Source - a table or read_X(...)</span>
                                    <input
                                        value={fromExpr}
                                        onChange={(e) => setFromExpr(e.target.value)}
                                        placeholder="read_parquet('data/sales.parquet')"
                                    />
                                </label>
                                <label className="dive-field">
                                    <span>Question</span>
                                    <input
                                        value={question}
                                        onChange={(e) => setQuestion(e.target.value)}
                                        placeholder="total revenue by region this year"
                                    />
                                </label>
                                <button
                                    className="dive-btn"
                                    onClick={() => void generate()}
                                    disabled={generating || !fromExpr.trim() || !question.trim()}
                                >
                                    {generating ? 'Generating…' : 'Generate'}
                                </button>
                                {genError ? (
                                    <div className="dive-panel-msg dive-panel-err">{genError}</div>
                                ) : null}
                            </div>
                            <label className="dive-field">
                                <span>Title</span>
                                <input value={title} onChange={(e) => setTitle(e.target.value)} />
                            </label>
                            <label className="dive-field">
                                <span>
                                    SQL - a single SELECT that reads its source inline, e.g.{' '}
                                    <code>FROM read_parquet('data/x.parquet')</code>
                                </span>
                                <textarea
                                    value={sql}
                                    onChange={(e) => setSql(e.target.value)}
                                    rows={6}
                                    spellCheck={false}
                                />
                            </label>
                            <div className="dive-editor-actions">
                                <button className="dive-btn" onClick={() => setPreview(draft())} disabled={!canRun}>
                                    Preview
                                </button>
                                <button
                                    className="dive-btn primary"
                                    onClick={() => onSave(title.trim() || 'Untitled dive', draft())}
                                    disabled={!canRun || !title.trim()}
                                >
                                    Save
                                </button>
                            </div>
                            {preview ? (
                                <DivePanel dive={preview} workspacePath={workspacePath} theme={theme} />
                            ) : null}
                        </div>
                    ) : preview ? (
                        <DivePanel dive={preview} workspacePath={workspacePath} theme={theme} />
                    ) : null}
                </div>
            </div>
        </div>
    );
}
