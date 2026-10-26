/*
 * A drafting-style sketch of the task loop for the section header: six
 * numbered stations on one rule, labelled above, and a dashed return path
 * below that brings the next message back to the start.
 */

const stations = ["01", "02", "03", "04", "05", "06"] as const;
const step = 64;
const x0 = 14;
const lineY = 30;
const returnY = 62;

/** The six stages as a loop; decorative, the grid below carries the content. */
export function LoopDiagram({ className }: { className?: string }) {
  const last = x0 + step * (stations.length - 1);
  const mid = (x0 + last) / 2;

  return (
    <svg
      aria-hidden
      width={last + x0}
      height={returnY + 10}
      viewBox={`0 0 ${last + x0} ${returnY + 10}`}
      className={className}
      fill="none"
    >
      {/* The forward rule between stations. */}
      <path d={`M${x0} ${lineY}H${last}`} stroke="var(--line)" />

      {/* The dashed return path, from the last station back to the first. */}
      <path
        d={`M${last} ${lineY + 7}V${returnY - 8}Q${last} ${returnY} ${last - 8} ${returnY}H${x0 + 8}Q${x0} ${returnY} ${x0} ${returnY - 8}V${lineY + 7}`}
        stroke="var(--faint)"
        strokeDasharray="3 3"
      />
      <path d={`M${x0 - 3.5} ${lineY + 10.5}L${x0} ${lineY + 6.5}L${x0 + 3.5} ${lineY + 10.5}`} stroke="var(--ink-2)" />

      {/* Knock out the dashes behind the return label. */}
      <rect x={mid - 46} y={returnY - 7} width={92} height={14} fill="var(--paper)" />
      <text
        x={mid}
        y={returnY + 3.5}
        textAnchor="middle"
        className="font-mono"
        fontSize="10"
        letterSpacing="0.08em"
        fill="var(--faint)"
      >
        NEXT MESSAGE
      </text>

      {stations.map((label, i) => {
        const x = x0 + step * i;
        return (
          <g key={label}>
            <circle cx={x} cy={lineY} r={3.5} fill={i === 0 ? "var(--ink)" : "var(--paper)"} stroke="var(--ink)" />
            <text
              x={x}
              y={lineY - 13}
              textAnchor="middle"
              className="font-mono"
              fontSize="10"
              letterSpacing="0.04em"
              fill="var(--muted)"
            >
              {label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
