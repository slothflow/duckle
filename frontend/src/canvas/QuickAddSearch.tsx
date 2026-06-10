import { useEffect, useMemo, useRef, useState } from 'react';
import { searchComponents, type ComponentDef } from '../workflow-ui/palette-data';

// Canvas quick-add: start typing on the canvas to fuzzy-search every component
// (sources, transforms, sinks, connectors, control, quality, code) and drop the
// match where your cursor is - no need to hunt through the palette.
export function QuickAddSearch({
    initialQuery,
    onPick,
    onClose,
}: {
    initialQuery: string;
    onPick: (component: ComponentDef) => void;
    onClose: () => void;
}) {
    const [query, setQuery] = useState(initialQuery);
    const [active, setActive] = useState(0);
    const inputRef = useRef<HTMLInputElement>(null);

    const results = useMemo(() => searchComponents(query), [query]);

    useEffect(() => {
        const el = inputRef.current;
        if (!el) return;
        el.focus();
        // Caret at the end so the seeded first character isn't selected.
        const n = el.value.length;
        el.setSelectionRange(n, n);
    }, []);

    // Reset the highlight to the top match whenever the query changes.
    useEffect(() => {
        setActive(0);
    }, [query]);

    const onKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Escape') {
            e.preventDefault();
            onClose();
        } else if (e.key === 'ArrowDown') {
            e.preventDefault();
            setActive(a => Math.min(a + 1, results.length - 1));
        } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            setActive(a => Math.max(a - 1, 0));
        } else if (e.key === 'Enter') {
            e.preventDefault();
            const c = results[active];
            if (c) onPick(c);
        }
    };

    return (
        <div className="quick-add-backdrop" onMouseDown={onClose}>
            <div className="quick-add" onMouseDown={e => e.stopPropagation()}>
                <input
                    ref={inputRef}
                    className="quick-add-input"
                    value={query}
                    placeholder="Search components..."
                    onChange={e => setQuery(e.target.value)}
                    onKeyDown={onKeyDown}
                    spellCheck={false}
                    aria-label="Add a component"
                />
                <div className="quick-add-list" role="listbox">
                    {results.length === 0 ? (
                        <div className="quick-add-empty">No components match "{query}".</div>
                    ) : (
                        results.map((c, i) => (
                            <button
                                key={c.id}
                                type="button"
                                role="option"
                                aria-selected={i === active}
                                className={'quick-add-row' + (i === active ? ' is-active' : '')}
                                onMouseEnter={() => setActive(i)}
                                onClick={() => onPick(c)}
                            >
                                <span className={'quick-add-dot kind-' + c.kind} aria-hidden="true" />
                                <span className="quick-add-label">{c.label}</span>
                                <span className="quick-add-id">{c.id}</span>
                                {c.availability !== 'available' ? (
                                    <span className="quick-add-tag">{c.availability}</span>
                                ) : null}
                            </button>
                        ))
                    )}
                </div>
                <div className="quick-add-hint">Enter to add &middot; Esc to close</div>
            </div>
        </div>
    );
}
