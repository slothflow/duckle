import { useState } from 'react';
import { createPortal } from 'react-dom';
import { SlidersHorizontal, X } from 'lucide-react';

type Props = {
    paramNames: string[];
    pipelineName: string;
    onSubmit: (values: Record<string, string>) => void;
    onCancel: () => void;
};

// Issue #127: when a pipeline references ${name} placeholders that no context
// or builtin resolves, prompt for their values right before the run. Blank
// fields are omitted so the engine/context fallback still applies. This is the
// editor counterpart of the web-dashboard run-parameters form, and runs on both
// the desktop app and the self-hosted web editor (one shared handler).
export default function RunParametersModal({ paramNames, pipelineName, onSubmit, onCancel }: Props) {
    const [values, setValues] = useState<Record<string, string>>({});

    const handleBackdrop = (e: React.MouseEvent) => {
        if (e.target === e.currentTarget) onCancel();
    };

    const submit = () => {
        const out: Record<string, string> = {};
        for (const name of paramNames) {
            const v = (values[name] ?? '').trim();
            if (v) out[name] = v;
        }
        onSubmit(out);
    };

    return createPortal(
        <div className="modal-backdrop" onClick={handleBackdrop}>
            <div className="modal">
                <div className="modal-header">
                    <div className="modal-title-row">
                        <SlidersHorizontal size={16} className="modal-title-icon" />
                        <div>
                            <div className="modal-title">Run parameters</div>
                            <div className="modal-subtitle">
                                Pipeline: <b>{pipelineName}</b>
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
                    <p style={{ margin: '0 0 12px', fontSize: 13, opacity: 0.8 }}>
                        This pipeline references variables that no context provides. Set a value for
                        this run, or leave a field blank to keep the placeholder unresolved.
                    </p>
                    <form
                        onSubmit={e => {
                            e.preventDefault();
                            submit();
                        }}
                        style={{ display: 'flex', flexDirection: 'column', gap: 10 }}
                    >
                        {paramNames.map((name, i) => (
                            <label
                                key={name}
                                style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13 }}
                            >
                                <span style={{ fontWeight: 600 }}>{name}</span>
                                <input
                                    className="modal-input"
                                    value={values[name] ?? ''}
                                    onChange={ev =>
                                        setValues(v => ({ ...v, [name]: ev.target.value }))
                                    }
                                    placeholder={'${' + name + '}'}
                                    spellCheck={false}
                                    autoFocus={i === 0}
                                />
                            </label>
                        ))}
                    </form>
                </div>

                <div className="modal-footer">
                    <button type="button" className="btn btn-secondary" onClick={onCancel}>
                        Cancel
                    </button>
                    <button type="button" className="btn btn-primary" onClick={submit}>
                        Run
                    </button>
                </div>
            </div>
        </div>,
        document.body,
    );
}
