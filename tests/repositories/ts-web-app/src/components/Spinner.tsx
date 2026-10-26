export interface SpinnerProps {
  /** Pixel size of the spinner. Defaults to 16. */
  size?: number;
  /** Accessible label announced by screen readers. */
  label?: string;
}

/**
 * Minimal inline loading indicator. Pure presentational component; it has no
 * state and no side effects, so it is safe to render anywhere.
 */
export function Spinner({ size = 16, label = "Loading" }: SpinnerProps) {
  const style = { width: size, height: size };
  return (
    <span className="spinner" role="status" aria-label={label} style={style}>
      <span className="spinner-ring" />
    </span>
  );
}
