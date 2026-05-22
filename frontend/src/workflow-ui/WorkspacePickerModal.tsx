import { useState } from 'react';
import { createPortal } from 'react-dom';
import { Folder, FolderOpen, Workflow } from 'lucide-react';
import { pickWorkspaceDirectory } from '../workspace';

type Props = {
    onPicked: (path: string) => void;
};

export default function WorkspacePickerModal({ onPicked }: Props) {
    const [picking, setPicking] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const handlePick = async () => {
        setPicking(true);
        setError(null);
        try {
            const path = await pickWorkspaceDirectory();
            if (path) {
                onPicked(path);
            } else {
                setError('No folder selected. Pick a directory where Duckle can store pipelines, connections, and routines.');
            }
        } catch (err) {
            setError(String(err));
        } finally {
            setPicking(false);
        }
    };

    return createPortal(
        <div className="modal-backdrop modal-backdrop-blocking">
            <div className="modal modal-workspace">
                <div className="modal-header modal-workspace-header">
                    <div className="modal-workspace-mark">
                        <Workflow size={28} />
                    </div>
                    <div className="modal-workspace-titles">
                        <div className="modal-title">Welcome to Duckle</div>
                        <div className="modal-subtitle">Pick a folder to store your workspace</div>
                    </div>
                </div>

                <div className="modal-body modal-workspace-body">
                    <p className="modal-workspace-lead">
                        Duckle keeps your pipelines, saved connections, contexts, routines, and
                        documentation in a regular folder on disk. Everything is plain JSON so it
                        plays nicely with version control.
                    </p>

                    <div className="modal-workspace-features">
                        <Feature
                            icon={<Folder size={14} />}
                            title="Pick an existing folder"
                            description="If you already have a Duckle workspace, point at the same folder and your saved state loads automatically."
                        />
                        <Feature
                            icon={<FolderOpen size={14} />}
                            title="Or choose a new empty folder"
                            description="Duckle will create workspace.json on first save. Use any folder - a fresh one, your Documents folder, anywhere."
                        />
                    </div>

                    {error ? <div className="modal-workspace-error">{error}</div> : null}
                </div>

                <div className="modal-footer modal-workspace-footer">
                    <button
                        type="button"
                        className="btn btn-primary"
                        onClick={handlePick}
                        disabled={picking}
                    >
                        {picking ? 'Opening picker…' : 'Choose workspace folder…'}
                    </button>
                </div>
            </div>
        </div>,
        document.body,
    );
}

function Feature({
    icon,
    title,
    description,
}: {
    icon: React.ReactNode;
    title: string;
    description: string;
}) {
    return (
        <div className="modal-workspace-feature">
            <div className="modal-workspace-feature-icon">{icon}</div>
            <div>
                <div className="modal-workspace-feature-title">{title}</div>
                <div className="modal-workspace-feature-desc">{description}</div>
            </div>
        </div>
    );
}
