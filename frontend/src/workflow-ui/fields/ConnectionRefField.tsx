import { useContext, useMemo } from 'react';
import { Plug } from 'lucide-react';
import type { Field } from './types';
import { FieldContext } from './FieldContext';
import type { ConnectionPayload } from '../../repo-types';

type Props = {
    field: Field;
    value: string | undefined;
    onChange: (v: string) => void;
};

export function ConnectionRefField({ field, value, onChange }: Props) {
    const { repoItems, onPickConnection } = useContext(FieldContext);

    const connections = useMemo(() => {
        const items = repoItems.filter(i => i.type === 'connection');
        if (!field.accepts || field.accepts.length === 0) return items;
        const allowed = new Set(field.accepts);
        return items.filter(i => {
            const payload = i.payload as ConnectionPayload | undefined;
            return !payload?.kind || allowed.has(payload.kind);
        });
    }, [repoItems, field.accepts]);

    if (connections.length === 0) {
        return (
            <div className="field-ref-empty">
                <Plug size={12} />
                <span>
                    No saved connections{field.accepts ? ' of compatible type' : ''}. Create one in
                    the <b>Connections</b> folder.
                </span>
            </div>
        );
    }

    const handleChange = (id: string) => {
        onChange(id);
        if (id && onPickConnection) {
            const item = connections.find(c => c.id === id);
            if (item?.payload) onPickConnection(item.payload as ConnectionPayload);
        }
    };

    return (
        <select
            className="field-input field-select"
            value={value ?? ''}
            onChange={e => handleChange(e.target.value)}
        >
            <option value="">- pick a saved connection -</option>
            {connections.map(c => {
                const payload = c.payload as ConnectionPayload | undefined;
                return (
                    <option key={c.id} value={c.id}>
                        {c.name} {payload?.kind ? '· ' + payload.kind : ''}
                    </option>
                );
            })}
        </select>
    );
}
