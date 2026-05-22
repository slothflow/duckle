import { useContext, useMemo } from 'react';
import { Code2 } from 'lucide-react';
import type { Field } from './types';
import { FieldContext } from './FieldContext';
import type { RoutinePayload } from '../../repo-types';

type Props = {
    field: Field;
    value: string | undefined;
    onChange: (v: string) => void;
};

export function RoutineRefField({ field, value, onChange }: Props) {
    const { repoItems, onPickRoutine } = useContext(FieldContext);

    const routines = useMemo(() => {
        const items = repoItems.filter(i => i.type === 'routine');
        if (!field.accepts || field.accepts.length === 0) return items;
        const allowed = new Set(field.accepts);
        return items.filter(i => {
            const payload = i.payload as RoutinePayload | undefined;
            return !payload?.language || allowed.has(payload.language);
        });
    }, [repoItems, field.accepts]);

    if (routines.length === 0) {
        return (
            <div className="field-ref-empty">
                <Code2 size={12} />
                <span>
                    No saved routines{field.accepts ? ' in compatible languages' : ''}. Create one
                    in the <b>Routines</b> folder.
                </span>
            </div>
        );
    }

    const handleChange = (id: string) => {
        onChange(id);
        if (id && onPickRoutine) {
            const item = routines.find(r => r.id === id);
            if (item?.payload) onPickRoutine(item.payload as RoutinePayload);
        }
    };

    return (
        <select
            className="field-input field-select"
            value={value ?? ''}
            onChange={e => handleChange(e.target.value)}
        >
            <option value="">- pick a saved routine -</option>
            {routines.map(r => {
                const payload = r.payload as RoutinePayload | undefined;
                return (
                    <option key={r.id} value={r.id}>
                        {r.name} {payload?.language ? '· ' + payload.language : ''}
                    </option>
                );
            })}
        </select>
    );
}
