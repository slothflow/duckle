// Duckle brand mark: the two-tone lowercase "d" - a peach bowl ring, an orange
// ascender stem, and the deeper overlap where they cross. Inline SVG so it stays
// crisp at any size and reads on both dark and light surfaces. Decorative by
// default - the adjacent "Duckle" wordmark carries the accessible name.
export function DuckleLogo({ size = 24, className }: { size?: number; className?: string }) {
    return (
        <svg
            width={size}
            height={size}
            viewBox="0 0 64 64"
            className={className ? `duckle-logo ${className}` : 'duckle-logo'}
            aria-hidden="true"
            focusable="false"
        >
            <defs>
                <clipPath id="duckle-logo-bowl">
                    <circle cx="31.5" cy="39.5" r="20.5" />
                </clipPath>
            </defs>
            <path
                fill="#F6BA78"
                fillRule="evenodd"
                d="M11,39.5 A20.5,20.5 0 1,0 52,39.5 A20.5,20.5 0 1,0 11,39.5 Z M20.4,39.5 A11.1,11.1 0 1,0 42.6,39.5 A11.1,11.1 0 1,0 20.4,39.5 Z"
            />
            <rect x="42.6" y="4" width="10.4" height="56" rx="5.2" fill="#EA7E42" />
            <rect x="42.6" y="4" width="10.4" height="56" rx="5.2" fill="#D9742F" fillOpacity="0.45" clipPath="url(#duckle-logo-bowl)" />
        </svg>
    );
}
