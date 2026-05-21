import { useState } from 'react';
import type { Field } from './types';
import { isTauri, pickFile, pickSavePath } from '../../tauri-dialog';

type Props = {
    field: Field;
    value: string | undefined;
    onChange: (v: string) => void;
    mode: 'open' | 'save';
};

export function FilePathField({ field, value, onChange, mode }: Props) {
    const [picking, setPicking] = useState(false);
    const tauri = isTauri();

    const handleBrowse = async () => {
        setPicking(true);
        try {
            const path =
                mode === 'open'
                    ? await pickFile({ filters: field.filters, title: field.label })
                    : await pickSavePath({
                          filters: field.filters,
                          defaultPath: value,
                          title: field.label,
                      });
            if (path) onChange(path);
        } finally {
            setPicking(false);
        }
    };

    return (
        <div className="field-path">
            <input
                type="text"
                className="field-input"
                value={value ?? ''}
                placeholder={field.placeholder ?? (mode === 'open' ? 'Path to file' : 'Output path')}
                onChange={e => onChange(e.target.value)}
                spellCheck={false}
            />
            <button
                type="button"
                className="field-path-browse"
                onClick={handleBrowse}
                disabled={picking}
                title={tauri ? 'Open native file picker' : 'Browser picker (path is filename only)'}
            >
                {picking ? '…' : mode === 'save' ? 'Save as…' : 'Browse…'}
            </button>
        </div>
    );
}
