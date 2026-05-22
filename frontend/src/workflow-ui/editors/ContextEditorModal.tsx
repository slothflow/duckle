import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Lock, Plus, Save, Variable, X } from 'lucide-react';
import type { ContextPayload, ContextVariable, RepoItem } from '../../repo-types';

type Props = {
    item: RepoItem | null;
    onSave: (name: string, payload: ContextPayload) => void;
    onCancel: () => void;
};

export default function ContextEditorModal({ item, onSave, onCancel }: Props) {
    const initial = (item?.payload as ContextPayload | undefined) ?? null;
    const [name, setName] = useState(item?.name ?? '');
    const [variables, setVariables] = useState<ContextVariable[]>(initial?.variables ?? []);
    const [description, setDescription] = useState(initial?.description ?? '');
    const nameRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        setTimeout(() => nameRef.current?.focus(), 30);
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onCancel();
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [onCancel]);

    const addVar = () => setVariables(vs => [...vs, { key: '', value: '' }]);
    const updateVar = (i: number, patch: Partial<ContextVariable>) =>
        setVariables(vs => vs.map((v, idx) => (idx === i ? { ...v, ...patch } : v)));
    const removeVar = (i: number) => setVariables(vs => vs.filter((_, idx) => idx !== i));

    const canSave = name.trim().length > 0;

    const handleSave = () => {
        if (!canSave) return;
        onSave(name.trim(), {
            variables: variables.filter(v => v.key.trim()),
            description: description.trim() || undefined,
        });
    };

    return createPortal(
        <div
            className="modal-backdrop"
            onClick={e => {
                if (e.target === e.currentTarget) onCancel();
            }}
        >
            <div className="modal modal-editor">
                <div className="modal-header">
                    <div className="modal-title-row">
                        <Variable size={16} className="modal-title-icon" />
                        <div>
                            <div className="modal-title">
                                {item ? 'Edit context' : 'New context'}
                            </div>
                            <div className="modal-subtitle">
                                Reusable variable set (dev / prod / region / …)
                            </div>
                        </div>
                    </div>
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
                        <label className="modal-field-label">Context name</label>
                        <input
                            ref={nameRef}
                            type="text"
                            className="modal-input"
                            value={name}
                            placeholder="e.g. prod, dev, us_west_2"
                            onChange={e => setName(e.target.value)}
                            spellCheck={false}
                        />
                    </div>

                    <div className="modal-field">
                        <label className="modal-field-label">Description (optional)</label>
                        <input
                            type="text"
                            className="modal-input"
                            value={description}
                            placeholder="What is this context for?"
                            onChange={e => setDescription(e.target.value)}
                            spellCheck={false}
                        />
                    </div>

                    <div className="modal-field">
                        <div className="modal-field-row">
                            <label className="modal-field-label">Variables</label>
                            <button type="button" className="schema-add" onClick={addVar}>
                                <Plus size={11} /> Add variable
                            </button>
                        </div>
                        {variables.length === 0 ? (
                            <div className="ctx-empty">
                                No variables yet. Click <b>Add variable</b> to define one.
                            </div>
                        ) : (
                            <div className="ctx-table">
                                <div className="ctx-row ctx-header">
                                    <div>Key</div>
                                    <div>Value</div>
                                    <div>Secret</div>
                                    <div />
                                </div>
                                {variables.map((v, i) => (
                                    <div className="ctx-row" key={i}>
                                        <input
                                            type="text"
                                            className="schema-input"
                                            value={v.key}
                                            placeholder="DB_HOST"
                                            onChange={e => updateVar(i, { key: e.target.value })}
                                            spellCheck={false}
                                        />
                                        <input
                                            type={v.secret ? 'password' : 'text'}
                                            className="schema-input"
                                            value={v.value}
                                            placeholder={v.secret ? '••••••••' : 'value'}
                                            onChange={e => updateVar(i, { value: e.target.value })}
                                            spellCheck={false}
                                        />
                                        <label
                                            className="ctx-secret-toggle"
                                            title="Treat as secret (masked, never logged)"
                                        >
                                            <input
                                                type="checkbox"
                                                checked={v.secret ?? false}
                                                onChange={e =>
                                                    updateVar(i, { secret: e.target.checked })
                                                }
                                            />
                                            <Lock size={11} />
                                        </label>
                                        <button
                                            type="button"
                                            className="schema-remove"
                                            onClick={() => removeVar(i)}
                                            aria-label="Remove"
                                        >
                                            <X size={11} />
                                        </button>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>

                    <div className="modal-tip" style={{ marginTop: 8 }}>
                        <span>
                            Reference variables in component properties with{' '}
                            <code>${'{ctx.KEY_NAME}'}</code> - they're resolved at pipeline run
                            time.
                        </span>
                    </div>
                </div>

                <div className="modal-footer">
                    <button type="button" className="btn btn-secondary" onClick={onCancel}>
                        Cancel
                    </button>
                    <button
                        type="button"
                        className="btn btn-primary"
                        onClick={handleSave}
                        disabled={!canSave}
                    >
                        <Save size={13} />
                        Save
                    </button>
                </div>
            </div>
        </div>,
        document.body,
    );
}
