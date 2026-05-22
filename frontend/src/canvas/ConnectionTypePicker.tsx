import { useEffect } from 'react';
import { createPortal } from 'react-dom';
import {
    CONNECTION_TYPES,
    ROW_CONNECTIONS,
    TRIGGER_CONNECTIONS,
    type ConnectionType,
    type ConnectionTypeMeta,
} from './connection-types';

type Props = {
    position: { x: number; y: number };
    allowedTypes?: Set<ConnectionType>;
    onPick: (type: ConnectionType) => void;
    onCancel: () => void;
};

export default function ConnectionTypePicker({ position, allowedTypes, onPick, onCancel }: Props) {
    const rowItems = allowedTypes
        ? ROW_CONNECTIONS.filter(t => allowedTypes.has(t.id))
        : ROW_CONNECTIONS;
    const triggerItems = allowedTypes
        ? TRIGGER_CONNECTIONS.filter(t => allowedTypes.has(t.id))
        : TRIGGER_CONNECTIONS;
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onCancel();
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [onCancel]);

    return createPortal(
        <div
            className="connection-picker-backdrop"
            onMouseDown={e => {
                if (e.target === e.currentTarget) onCancel();
            }}
        >
            <div
                className="connection-picker"
                style={{
                    left: Math.min(position.x, window.innerWidth - 340),
                    top: Math.min(position.y, window.innerHeight - 460),
                }}
                onMouseDown={e => e.stopPropagation()}
            >
                <div className="connection-picker-header">
                    <span>Choose connection type</span>
                    <span className="connection-picker-hint">Esc to cancel</span>
                </div>
                {rowItems.length > 0 ? (
                    <PickerSection
                        title="Row connections"
                        subtitle="Carry tabular data between components"
                        items={rowItems}
                        onPick={onPick}
                    />
                ) : null}
                {triggerItems.length > 0 ? (
                    <PickerSection
                        title="Trigger connections"
                        subtitle="Control flow, no data - fires once when condition is met"
                        items={triggerItems}
                        onPick={onPick}
                    />
                ) : null}
            </div>
        </div>,
        document.body,
    );
}

function PickerSection({
    title,
    subtitle,
    items,
    onPick,
}: {
    title: string;
    subtitle: string;
    items: ConnectionTypeMeta[];
    onPick: (type: ConnectionType) => void;
}) {
    return (
        <div className="connection-picker-section">
            <div className="connection-picker-section-title">{title}</div>
            <div className="connection-picker-section-subtitle">{subtitle}</div>
            <div className="connection-picker-grid">
                {items.map(t => (
                    <button
                        key={t.id}
                        type="button"
                        className="connection-picker-item"
                        onClick={() => onPick(t.id)}
                    >
                        <svg
                            className="connection-picker-stroke"
                            width="40"
                            height="14"
                            viewBox="0 0 40 14"
                            aria-hidden="true"
                        >
                            <line
                                x1="2"
                                y1="7"
                                x2="38"
                                y2="7"
                                stroke={t.color}
                                strokeWidth={t.width + 0.4}
                                strokeDasharray={t.dash ?? undefined}
                                strokeLinecap="round"
                            />
                            <polygon
                                points="34,3 38,7 34,11"
                                fill={t.color}
                            />
                        </svg>
                        <div className="connection-picker-item-text">
                            <div className="connection-picker-item-label">{t.label}</div>
                            <div className="connection-picker-item-desc">{t.description}</div>
                        </div>
                        {t.badge ? (
                            <span className="connection-picker-item-badge">{t.badge}</span>
                        ) : null}
                    </button>
                ))}
            </div>
        </div>
    );
}

export { CONNECTION_TYPES };
