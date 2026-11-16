import type { Sample } from "./types";

const W = 312;
const H = 76;
const WINDOW_MS = 5 * 60 * 1000;
/** Samples further apart than this are drawn as separate segments. */
const GAP_MS = 5000;

interface Props {
  history: Sample[];
  now: number;
  color: string;
  /** Resting HR + 20; drawn as a dashed line when it falls inside the range. */
  threshold: number | null;
}

export default function Sparkline({ history, now, color, threshold }: Props) {
  if (history.length < 2) {
    return (
      <svg className="spark" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Heart rate, last 5 minutes">
        <line x1="0" x2={W} y1={H / 2} y2={H / 2} className="spark-empty" />
      </svg>
    );
  }

  const values = history.map(([, b]) => b);
  let lo = Math.min(...values) - 4;
  let hi = Math.max(...values) + 4;
  if (hi - lo < 24) {
    const mid = (hi + lo) / 2;
    lo = mid - 12;
    hi = mid + 12;
  }
  const x = (t: number) => ((t - (now - WINDOW_MS)) / WINDOW_MS) * W;
  const y = (b: number) => H - 3 - ((b - lo) / (hi - lo)) * (H - 6);

  let line = "";
  let area = "";
  let segmentStart = 0;
  history.forEach(([t, b], i) => {
    const px = x(t).toFixed(1);
    const py = y(b).toFixed(1);
    const gap = i > 0 && t - history[i - 1][0] > GAP_MS;
    if (i === 0 || gap) {
      if (gap) area += `L${x(history[i - 1][0]).toFixed(1)},${H} L${x(history[segmentStart][0]).toFixed(1)},${H} Z `;
      segmentStart = i;
      line += `M${px},${py} `;
      area += `M${px},${py} `;
    } else {
      line += `L${px},${py} `;
      area += `L${px},${py} `;
    }
  });
  const last = history[history.length - 1];
  area += `L${x(last[0]).toFixed(1)},${H} L${x(history[segmentStart][0]).toFixed(1)},${H} Z`;

  const thresholdY = threshold !== null && threshold > lo && threshold < hi ? y(threshold) : null;

  return (
    <svg className="spark" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Heart rate, last 5 minutes">
      <defs>
        <linearGradient id="spark-fill" x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor={color} stopOpacity="0.32" />
          <stop offset="1" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      {thresholdY !== null && (
        <line x1="0" x2={W} y1={thresholdY} y2={thresholdY} className="spark-threshold" />
      )}
      <path d={area} fill="url(#spark-fill)" />
      <path d={line} fill="none" stroke={color} strokeWidth="1.75" strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={x(last[0])} cy={y(last[1])} r="3" fill={color} />
    </svg>
  );
}
