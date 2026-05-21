type Pair = { key: string; value: string };

type Props = {
    value: Pair[] | undefined;
    onChange: (v: Pair[]) => void;
};

export function KeyValueField({ value, onChange }: Props) {
    const pairs = value ?? [];

    const add = () => onChange([...pairs, { key: '', value: '' }]);
    const update = (i: number, patch: Partial<Pair>) =>
        onChange(pairs.map((p, idx) => (idx === i ? { ...p, ...patch } : p)));
    const remove = (i: number) => onChange(pairs.filter((_, idx) => idx !== i));

    return (
        <div className="field-kv">
            <div className="field-agg-toolbar">
                <span className="field-agg-count">
                    {pairs.length} entr{pairs.length === 1 ? 'y' : 'ies'}
                </span>
                <button type="button" className="schema-add" onClick={add}>
                    + Add
                </button>
            </div>
            {pairs.map((p, i) => (
                <div className="field-kv-row" key={i}>
                    <input
                        type="text"
                        className="schema-input"
                        value={p.key}
                        placeholder="key"
                        onChange={e => update(i, { key: e.target.value })}
                        spellCheck={false}
                    />
                    <input
                        type="text"
                        className="schema-input"
                        value={p.value}
                        placeholder="value"
                        onChange={e => update(i, { value: e.target.value })}
                        spellCheck={false}
                    />
                    <button
                        type="button"
                        className="schema-remove"
                        onClick={() => remove(i)}
                        aria-label="Remove"
                    >
                        ×
                    </button>
                </div>
            ))}
        </div>
    );
}
