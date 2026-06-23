// Duckle brand mark: the exact lowercase "d" artwork. Rendered from the source
// PNG (object-fit: contain in a square box) so it stays pixel-faithful and is not
// a recreation. Decorative by default - the adjacent "Duckle" wordmark carries
// the accessible name.
import markUrl from './duckle-mark.png';

export function DuckleLogo({ size = 24, className }: { size?: number; className?: string }) {
    return (
        <img
            src={markUrl}
            width={size}
            height={size}
            className={className ? `duckle-logo ${className}` : 'duckle-logo'}
            alt=""
            aria-hidden="true"
            style={{ objectFit: 'contain' }}
        />
    );
}
