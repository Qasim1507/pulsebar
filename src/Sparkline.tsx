import type { Sample } from "./types";

const W = 312;
const H = 64;
const WINDOW_MS = 5 * 60 * 1000;
/** Samples further apart than this are drawn as separate segments. */
const GAP_MS = 5000;

export const CALM = "#3b82f6";
export const HIGH = "#ef4444";

interface Props {
  history: Sample[];
  now: number;
  /** Resting HR + 20. The line is red above it and blue below. */
  threshold: number | null;
}

/**
 * Gradient stops that switch colour at pixel row `splitY` (red above, blue
 * below). `opacity` gives the alpha at a 0–1 position down the chart.
 */
export function twoToneStops(splitY: number | null, height: number, allHigh: boolean, opacity: (p: number) => number) {
  if (splitY === null) {
    const color = allHigh ? HIGH : CALM;
    return [
      <stop key="a" offset="0" stopColor={color} stopOpacity={opacity(0)} />,
      <stop key="b" offset="1" stopColor={color} stopOpacity={opacity(1)} />,
    ];
  }
  const p = Math.min(1, Math.max(0, splitY / height));
  return [
    <stop key="a" offset="0" stopColor={HIGH} stopOpacity={opacity(0)} />,
    <stop key="b" offset={p} stopColor={HIGH} stopOpacity={opacity(p)} />,
    <stop key="c" offset={p} stopColor={CALM} stopOpacity={opacity(p)} />,
    <stop key="d" offset="1" stopColor={CALM} stopOpacity={opacity(1)} />,
  ];
}

export default function Sparkline({ history, now, threshold }: Props) {
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

  const inRange = threshold !== null && threshold > lo && threshold < hi;
  const splitY = inRange ? y(threshold) : null;
  const allHigh = threshold !== null && threshold <= lo;
  const lastHigh = threshold !== null && last[1] > threshold;

  return (
    <svg className="spark" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Heart rate, last 5 minutes">
      <defs>
        <linearGradient id="spark-line" gradientUnits="userSpaceOnUse" x1="0" x2="0" y1="0" y2={H}>
          {twoToneStops(splitY, H, allHigh, () => 1)}
        </linearGradient>
        <linearGradient id="spark-fill" gradientUnits="userSpaceOnUse" x1="0" x2="0" y1="0" y2={H}>
          {twoToneStops(splitY, H, allHigh, (p) => 0.42 * (1 - p) + 0.03)}
        </linearGradient>
      </defs>
      {splitY !== null && <line x1="0" x2={W} y1={splitY} y2={splitY} className="spark-threshold" />}
      <path d={area} fill="url(#spark-fill)" />
      <path d={line} fill="none" stroke="url(#spark-line)" strokeWidth="1.75" strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={x(last[0])} cy={y(last[1])} r="3" fill={lastHigh ? HIGH : CALM} />
    </svg>
  );
}
