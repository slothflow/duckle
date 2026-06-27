import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import type { RepoItem } from '../repo-types';

export type PipelineTemplate = 'empty' | 'sample-csv-to-parquet' | 'sample-join-groupby' | 'from-sql';

type Props = {
    open: boolean;
    defaultParentId: string;
    repoItems: RepoItem[];
    onCancel: () => void;
    onCreate: (name: string, parentId: string, template: PipelineTemplate) => void;
};

const TEMPLATES: { id: PipelineTemplate; label: string; description: string; available: boolean }[] = [
    {
        id: 'empty',
        label: 'Empty',
        description: 'Blank canvas. Drag components from the palette to start.',
        available: true,
    },
    {
        id: 'sample-csv-to-parquet',
        label: 'Sample · CSV → Filter → Parquet',
        description: 'Pre-wired three-node pipeline to learn the editor.',
        available: true,
    },
    {
        id: 'sample-join-groupby',
        label: 'Sample · Join + Group By',
        description: 'Join two CSVs and aggregate — uses xf.groupby (not xf.aggregate).',
        available: true,
    },
    {
        id: 'from-sql',
        label: 'From SQL',
        description: 'Paste a SELECT statement and generate the graph. (coming soon)',
        available: false,
    },
];

function buildPath(item: RepoItem, allItems: RepoItem[]): string {
    const chain: string[] = [];
    let current: RepoItem | undefined = item;
    while (current) {
        chain.unshift(current.name);
        current = current.parentId ? allItems.find(i => i.id === current!.parentId) : undefined;
    }
    return chain.join(' / ');
}

function sanitizeName(raw: string): string {
    return raw
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, '_')
        .replace(/^_+|_+$/g, '');
}

export default function NewPipelineModal(props: Props) {
    const { open, defaultParentId, repoItems, onCancel, onCreate } = props;
    const [name, setName] = useState('');
    const [parentId, setParentId] = useState(defaultParentId);
    const [template, setTemplate] = useState<PipelineTemplate>('empty');
    const nameRef = useRef<HTMLInputElement>(null);

    const folderOptions = useMemo(
        () =>
            repoItems
                .filter(i => i.type === 'project' || i.type === 'folder')
                .map(f => ({ id: f.id, label: buildPath(f, repoItems) })),
        [repoItems],
    );

    useEffect(() => {
        if (open) {
            setName('');
            setTemplate('empty');
            setParentId(defaultParentId);
            setTimeout(() => nameRef.current?.focus(), 30);
        }
    }, [open, defaultParentId]);

    useEffect(() => {
        if (!open) return;
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                e.preventDefault();
                onCancel();
            }
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [open, onCancel]);

    if (!open) return null;

    const sanitized = sanitizeName(name);
    const canCreate = sanitized.length > 0;

    const handleCreate = () => {
        if (!canCreate) return;
        onCreate(sanitized, parentId, template);
    };

    return createPortal(
        <div
            className="modal-backdrop"
            role="dialog"
            aria-modal="true"
            onClick={e => {
                if (e.target === e.currentTarget) onCancel();
            }}
        >
            <div className="modal modal-new-pipeline">
                <div className="modal-header">
                    <div className="modal-title">New pipeline</div>
                    <button
                        type="button"
                        className="modal-close"
                        onClick={onCancel}
                        aria-label="Close"
                    >
                        <X size={16} />
                    </button>
                </div>

                <div className="modal-body">
                    <div className="modal-field">
                        <label className="modal-field-label">Pipeline name</label>
                        <input
                            ref={nameRef}
                            type="text"
                            className="modal-input"
                            value={name}
                            placeholder="e.g. customer_360"
                            onChange={e => setName(e.target.value)}
                            onKeyDown={e => {
                                if (e.key === 'Enter' && canCreate) {
                                    e.preventDefault();
                                    handleCreate();
                                }
                            }}
                            spellCheck={false}
                        />
                        {name && name !== sanitized ? (
                            <div className="modal-field-hint">
                                Will be saved as <code>{sanitized}</code>
                            </div>
                        ) : null}
                    </div>

                    <div className="modal-field">
                        <label className="modal-field-label">Folder</label>
                        <select
                            className="modal-input modal-select"
                            value={parentId}
                            onChange={e => setParentId(e.target.value)}
                        >
                            {folderOptions.map(f => (
                                <option key={f.id} value={f.id}>
                                    {f.label}
                                </option>
                            ))}
                        </select>
                    </div>

                    <div className="modal-field">
                        <label className="modal-field-label">Template</label>
                        <div className="template-grid">
                            {TEMPLATES.map(t => (
                                <button
                                    key={t.id}
                                    type="button"
                                    className={
                                        'template-card' +
                                        (template === t.id ? ' is-active' : '') +
                                        (!t.available ? ' is-disabled' : '')
                                    }
                                    onClick={() => {
                                        if (t.available) setTemplate(t.id);
                                    }}
                                    disabled={!t.available}
                                >
                                    <div className="template-card-title">{t.label}</div>
                                    <div className="template-card-desc">{t.description}</div>
                                </button>
                            ))}
                        </div>
                    </div>
                </div>

                <div className="modal-footer">
                    <button type="button" className="btn btn-secondary" onClick={onCancel}>
                        Cancel
                    </button>
                    <button
                        type="button"
                        className="btn btn-primary"
                        onClick={handleCreate}
                        disabled={!canCreate}
                    >
                        Create
                    </button>
                </div>
            </div>
        </div>,
        document.body,
    );
}
