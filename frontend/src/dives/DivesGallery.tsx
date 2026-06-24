// A gallery of every dive and dashboard in the workspace, opened from the top
// bar so dives are discoverable without hunting through the project tree.
// Clicking a card opens it (the existing dive/dashboard modal). See
// docs/design/dives.md.

import { BarChart3, LayoutGrid, Plus, X } from 'lucide-react';
import type { RepoItem } from '../repo-types';

interface DivesGalleryProps {
    dives: RepoItem[];
    dashboards: RepoItem[];
    onOpenDive: (id: string) => void;
    onOpenDashboard: (id: string) => void;
    onNewDive: () => void;
    onNewDashboard: () => void;
    onClose: () => void;
}

export function DivesGallery({
    dives,
    dashboards,
    onOpenDive,
    onOpenDashboard,
    onNewDive,
    onNewDashboard,
    onClose,
}: DivesGalleryProps) {
    return (
        <div className="dive-modal-backdrop" onClick={onClose}>
            <div className="dives-gallery" onClick={(e) => e.stopPropagation()}>
                <div className="dives-gallery-head">
                    <h2 className="dives-gallery-title">Dives</h2>
                    <button type="button" className="dive-btn" onClick={onClose} aria-label="Close">
                        <X size={16} />
                    </button>
                </div>
                <p className="dives-gallery-sub">
                    Live, always-fresh data views - each re-runs its query when you open it.
                </p>

                <div className="dives-gallery-section">
                    <div className="dives-gallery-section-head">
                        <span>Dives</span>
                        <button type="button" className="dive-btn" onClick={onNewDive}>
                            <Plus size={14} /> New dive
                        </button>
                    </div>
                    {dives.length === 0 ? (
                        <p className="dive-panel-msg">No dives yet. Create one to get started.</p>
                    ) : (
                        <div className="dives-gallery-grid">
                            {dives.map((d) => (
                                <button
                                    key={d.id}
                                    type="button"
                                    className="dives-gallery-card"
                                    onClick={() => onOpenDive(d.id)}
                                >
                                    <BarChart3 size={20} className="dives-gallery-card-icon" />
                                    <span className="dives-gallery-card-name">{d.name}</span>
                                </button>
                            ))}
                        </div>
                    )}
                </div>

                <div className="dives-gallery-section">
                    <div className="dives-gallery-section-head">
                        <span>Dashboards</span>
                        <button type="button" className="dive-btn" onClick={onNewDashboard}>
                            <Plus size={14} /> New dashboard
                        </button>
                    </div>
                    {dashboards.length === 0 ? (
                        <p className="dive-panel-msg">No dashboards yet.</p>
                    ) : (
                        <div className="dives-gallery-grid">
                            {dashboards.map((d) => (
                                <button
                                    key={d.id}
                                    type="button"
                                    className="dives-gallery-card"
                                    onClick={() => onOpenDashboard(d.id)}
                                >
                                    <LayoutGrid size={20} className="dives-gallery-card-icon" />
                                    <span className="dives-gallery-card-name">{d.name}</span>
                                </button>
                            ))}
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
