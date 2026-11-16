import { useEffect, useState } from "react";
import { call } from "./api";
import type { DayHistory } from "./types";

const DAY_MS = 24 * 60 * 60 * 1000;
const W = 312;
const H = 120;
const PAD_BOTTOM = 14;
/** Minute buckets further apart than this start a new line segment. */
const GAP_MS = 3 * 60 * 1000;

function startOfDay(offset: number): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - offset);
  return d.getTime();
}

const clock = (t: number) => new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

function duration(ms: number): string {
  const mins = Math.max(1, Math.round(ms / 60000));
  return mins < 60 ? `${mins} min` : `${Math.floor(mins / 60)} h ${mins % 60} min`;
}

export default function History({ rhr }: { rhr: number | null }) {
  // 0 = today, 6 = six days ago; older data is deleted automatically.
  const [offset, setOffset] = useState(0);
  const [day, setDay] = useState<DayHistory | null>(null);
  const [error, setError] = useState<string | null>(null);

  const from = startOfDay(offset);
  useEffect(() => {
    let alive = true;
    const load = () =>
      call<DayHistory>("get_history", { from, to: from + DAY_MS })
        .then((d) => alive && (setDay(d), setError(null)))
        .catch((e) => alive && setError(String(e)));
    load();
    // Today keeps growing; refresh it once a minute.
    const id = offset === 0 ? setInterval(load, 60_000) : undefined;
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [from, offset]);

  const points = day?.points ?? [];
  const episodes = day?.episodes ?? [];

  let lo = 40;
  let hi = 120;
  if (points.length) {
    lo = Math.floor((Math.min(...points.map((p) => p.min)) - 5) / 10) * 10;
    hi = Math.ceil((Math.max(...points.map((p) => p.max)) + 5) / 10) * 10;
  }
  const x = (t: number) => ((t - from) / DAY_MS) * W;
  const y = (b: number) => (H - PAD_BOTTOM) * (1 - (b - lo) / (hi - lo));

  // Split into runs so gaps (strap off, laptop asleep) aren't bridged.
  const runs: (typeof points)[] = [];
  points.forEach((p, i) => {
    if (i === 0 || p.t - points[i - 1].t > GAP_MS) runs.push([]);
    runs[runs.length - 1].push(p);
  });
  const line = (run: typeof points) => run.map((p, i) => `${i ? "L" : "M"}${x(p.t).toFixed(1)},${y(p.avg).toFixed(1)}`).join(" ");
  const band = (run: typeof points) =>
    run.map((p, i) => `${i ? "L" : "M"}${x(p.t).toFixed(1)},${y(p.max).toFixed(1)}`).join(" ") +
    " " +
    [...run].reverse().map((p) => `L${x(p.t).toFixed(1)},${y(p.min).toFixed(1)}`).join(" ") +
    " Z";

  const avg = points.length ? Math.round(points.reduce((a, p) => a + p.avg, 0) / points.length) : null;
  const min = points.length ? Math.min(...points.map((p) => p.min)) : null;
  const max = points.length ? Math.max(...points.map((p) => p.max)) : null;
  const stressTotal = episodes.reduce((a, e) => a + (e.end - e.start), 0);
  const threshold = rhr !== null && rhr + 20 > lo && rhr + 20 < hi ? rhr + 20 : null;

  return (
    <main className="content history">
      <nav className="days">
        {[6, 5, 4, 3, 2, 1, 0].map((o) => {
          const d = new Date(startOfDay(o));
          return (
            <button key={o} className={o === offset ? "active" : ""} onClick={() => setOffset(o)}>
              {o === 0 ? "Today" : d.toLocaleDateString([], { weekday: "short" })}
              <strong>{d.getDate()}</strong>
            </button>
          );
        })}
      </nav>

      <section className="card">
        <svg className="day-chart" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Heart rate across the day">
          {[lo, (lo + hi) / 2, hi].map((v) => (
            <g key={v}>
              <line className="grid" x1="0" x2={W} y1={y(v)} y2={y(v)} />
              <text x={W} y={y(v) - 2} textAnchor="end">
                {Math.round(v)}
              </text>
            </g>
          ))}
          {[0, 6, 12, 18, 24].map((h) => (
            <text key={h} x={(h / 24) * W} y={H - 2} textAnchor={h === 0 ? "start" : h === 24 ? "end" : "middle"}>
              {h === 0 || h === 24 ? "12a" : h === 12 ? "12p" : h < 12 ? `${h}a` : `${h - 12}p`}
            </text>
          ))}
          {episodes.map((e) => (
            <rect
              key={e.start}
              className="episode"
              x={x(e.start)}
              y="0"
              width={Math.max(1.5, x(e.end) - x(e.start))}
              height={H - PAD_BOTTOM}
            />
          ))}
          {threshold !== null && (
            <line className="spark-threshold" x1="0" x2={W} y1={y(threshold)} y2={y(threshold)} />
          )}
          {runs.map((run) => (
            <g key={run[0].t}>
              <path className="band" d={band(run)} />
              <path className="avg" d={line(run)} />
            </g>
          ))}
        </svg>

        {points.length === 0 ? (
          <p className="empty" style={{ marginTop: 10 }}>
            {error ?? "No heart rate recorded this day. History is saved while a strap is connected."}
          </p>
        ) : (
          <dl className="day-stats">
            <div>
              <dt>Min</dt>
              <dd>{min}</dd>
            </div>
            <div>
              <dt>Avg</dt>
              <dd>{avg}</dd>
            </div>
            <div>
              <dt>Max</dt>
              <dd>{max}</dd>
            </div>
            <div>
              <dt>Stressed</dt>
              <dd>{episodes.length ? duration(stressTotal).replace(" min", "m") : "0m"}</dd>
            </div>
          </dl>
        )}
      </section>

      <section className="card">
        <h2>Desk stress episodes</h2>
        {episodes.length === 0 ? (
          <p className="empty">None this day.</p>
        ) : (
          <ul className="episodes">
            {episodes.map((e) => (
              <li key={e.start}>
                <span>
                  {clock(e.start)} · {duration(e.end - e.start)}
                </span>
                <span>peak {e.peak} bpm</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <footer className="foot">
        <span>Stored on this computer only · deleted after 7 days</span>
      </footer>
    </main>
  );
}
