import { useEffect, useState } from "react";
import { Flame, Gauge, Lock, Sparkles, Type, Wand2 } from "lucide-react";
import { api, type DailyPoint, type Stats, type StatsRange } from "../lib/api";

/** Average sustained typing speed — the benchmark the WPM gauge compares against. */
const TYPING_WPM = 50;
/** All-time words needed before the "Your Voice" profile unlocks. */
const VOICE_PROFILE_WORDS = 2000;
/** Gauge maxes out here (keeps a realistic dictation speed near full). */
const GAUGE_MAX_WPM = 200;

const RANGES: { value: StatsRange; label: string }[] = [
  { value: "day", label: "Today" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
  { value: "all", label: "All time" },
];

const CATEGORY_LABELS: Record<string, string> = {
  email: "Email",
  workmsg: "Work chat",
  personalmsg: "Personal chat",
  code: "Code",
  other: "Other",
};

const categoryLabel = (c: string) => CATEGORY_LABELS[c] ?? c;

/** Rough percentile vs. the typing population, by dictation WPM. */
function topPercentile(wpm: number): number {
  if (wpm >= 140) return 1;
  if (wpm >= 110) return 5;
  if (wpm >= 90) return 10;
  if (wpm >= 70) return 25;
  if (wpm >= TYPING_WPM) return 50;
  return 75;
}

export function InsightsPage() {
  const [range, setRange] = useState<StatsRange>("week");
  const [stats, setStats] = useState<Stats | null>(null);
  const [allStats, setAllStats] = useState<Stats | null>(null);

  useEffect(() => {
    api.getStats(range).then(setStats).catch(() => setStats(null));
  }, [range]);

  // The heatmap + Voice profile span all of history, independent of the range.
  useEffect(() => {
    api.getStats("all").then(setAllStats).catch(() => setAllStats(null));
  }, []);

  const minutes = stats ? stats.totalMs / 60000 : 0;
  const wpm = stats && minutes > 0 ? Math.round(stats.totalWords / minutes) : 0;
  const hasData = !!stats && stats.totalSessions > 0;

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl">Insights</h1>
        <div className="flex items-center gap-1 rounded-xl border border-border bg-surface p-1 text-xs">
          {RANGES.map((r) => (
            <button
              key={r.value}
              onClick={() => setRange(r.value)}
              className={
                "rounded-lg px-3 py-1.5 transition-colors " +
                (range === r.value
                  ? "bg-accent-soft font-medium text-ink"
                  : "text-ink-soft hover:bg-surface-2")
              }
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        How fast you speak, how clean your dictations come out, and where you use Eve.
      </p>

      {!stats ? (
        <p className="mt-8 text-sm text-ink-faint">Loading…</p>
      ) : !hasData ? (
        <EmptyState />
      ) : (
        <>
          <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2">
            <SpeedCard wpm={wpm} />
            <CleanlinessCard stats={stats} />
          </div>

          <AppUsageCard stats={stats} />
        </>
      )}

      <StreakCard daily={allStats?.daily ?? []} />
      <VoiceProfileCard stats={allStats} />
    </div>
  );
}

/** WPM radial gauge + a "Top X%" badge benchmarked against typing. */
function SpeedCard({ wpm }: { wpm: number }) {
  const frac = Math.max(0, Math.min(1, wpm / GAUGE_MAX_WPM));
  const r = 52;
  const c = 2 * Math.PI * r;
  const faster = wpm > 0 ? (wpm / TYPING_WPM).toFixed(1) : "0";

  return (
    <div className="rounded-2xl border border-border bg-surface p-5">
      <div className="flex items-center gap-1.5 text-xs text-ink-faint">
        <Gauge size={14} /> Speaking speed
      </div>
      <div className="mt-3 flex items-center gap-5">
        <div className="relative h-32 w-32 shrink-0">
          <svg viewBox="0 0 120 120" className="h-full w-full -rotate-90">
            <circle cx="60" cy="60" r={r} fill="none" strokeWidth="10" className="stroke-surface-2" />
            <circle
              cx="60"
              cy="60"
              r={r}
              fill="none"
              strokeWidth="10"
              strokeLinecap="round"
              className="stroke-accent transition-[stroke-dashoffset] duration-700"
              strokeDasharray={c}
              strokeDashoffset={c * (1 - frac)}
            />
          </svg>
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <span className="font-serif text-3xl tabular-nums leading-none">{wpm}</span>
            <span className="text-[10px] uppercase tracking-wide text-ink-faint">wpm</span>
          </div>
        </div>
        <div className="min-w-0">
          <div className="inline-flex items-center gap-1 rounded-full bg-accent-soft px-2.5 py-1 text-xs font-medium text-ink">
            <Sparkles size={12} className="text-accent" /> Top {topPercentile(wpm)}%
          </div>
          <p className="mt-2 text-sm text-ink-soft">
            <span className="font-medium text-ink">{faster}×</span> faster than the{" "}
            {TYPING_WPM} WPM typing average.
          </p>
        </div>
      </div>
    </div>
  );
}

/** Cleanup edits per 100 words — how much Eve had to fix your raw speech. */
function CleanlinessCard({ stats }: { stats: Stats }) {
  const per100 =
    stats.totalWords > 0 ? (stats.corrections / stats.totalWords) * 100 : 0;
  return (
    <div className="rounded-2xl border border-border bg-surface p-5">
      <div className="flex items-center gap-1.5 text-xs text-ink-faint">
        <Wand2 size={14} /> Cleanup
      </div>
      <div className="mt-3 font-serif text-3xl tabular-nums">
        {per100.toFixed(1)}
        <span className="ml-1 text-base text-ink-faint">/ 100 words</span>
      </div>
      <p className="mt-2 text-sm text-ink-soft">
        {stats.corrections.toLocaleString()} cleanup edits across{" "}
        {stats.totalWords.toLocaleString()} words — fillers, punctuation, dictionary
        fixes, and polish.
      </p>
    </div>
  );
}

/** Horizontal bar breakdown of dictations by focused-app category. */
function AppUsageCard({ stats }: { stats: Stats }) {
  const usage = stats.appUsage;
  if (usage.length === 0) return null;
  const max = Math.max(...usage.map((u) => u.sessions), 1);
  return (
    <section className="mt-4 rounded-2xl border border-border bg-surface p-5">
      <div className="flex items-center gap-1.5 text-xs text-ink-faint">
        <Type size={14} /> Where you dictate
      </div>
      <div className="mt-4 space-y-3">
        {usage.map((u) => (
          <div key={u.category}>
            <div className="mb-1 flex items-center justify-between text-sm">
              <span className="text-ink-soft">{categoryLabel(u.category)}</span>
              <span className="font-mono text-xs text-ink-faint">
                {u.sessions} · {u.words.toLocaleString()} words
              </span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-surface-2">
              <div
                className="h-full rounded-full bg-accent transition-[width] duration-500"
                style={{ width: `${(u.sessions / max) * 100}%` }}
              />
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

const WEEKS = 13;

type Cell = { date: string; words: number; future: boolean };

function localKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** A `WEEKS`-wide grid of days ending in the current week (most recent column). */
function buildHeatmap(daily: DailyPoint[]): Cell[] {
  const map = new Map(daily.map((d) => [d.date, d.words]));
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const total = WEEKS * 7;
  // Align so the final column is the current (partial) week: pad to Saturday.
  const trailing = 6 - today.getDay();
  const start = new Date(today);
  start.setDate(today.getDate() - (total - 1 - trailing));
  const cells: Cell[] = [];
  for (let i = 0; i < total; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    const key = localKey(d);
    cells.push({ date: key, words: map.get(key) ?? 0, future: d > today });
  }
  return cells;
}

/** Current run of consecutive days (ending today or yesterday) with any words. */
function currentStreak(daily: DailyPoint[]): number {
  const active = new Set(daily.filter((d) => d.words > 0).map((d) => d.date));
  const cur = new Date();
  cur.setHours(0, 0, 0, 0);
  // Allow the streak to "hold" if today is empty but yesterday was active.
  if (!active.has(localKey(cur))) cur.setDate(cur.getDate() - 1);
  let streak = 0;
  while (active.has(localKey(cur))) {
    streak++;
    cur.setDate(cur.getDate() - 1);
  }
  return streak;
}

function StreakCard({ daily }: { daily: DailyPoint[] }) {
  const cells = buildHeatmap(daily);
  const max = Math.max(...cells.map((c) => c.words), 1);
  const streak = currentStreak(daily);

  const level = (words: number): number => {
    if (words <= 0) return 0;
    return Math.min(4, Math.ceil((words / max) * 4));
  };
  const fill = ["bg-surface-2", "bg-accent/25", "bg-accent/45", "bg-accent/70", "bg-accent"];

  return (
    <section className="mt-4 rounded-2xl border border-border bg-surface p-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-xs text-ink-faint">
          <Flame size={14} /> Daily streak
        </div>
        <div className="text-xs text-ink-soft">
          <span className="font-medium text-ink">{streak}</span>{" "}
          {streak === 1 ? "day" : "days"} in a row
        </div>
      </div>
      <div className="mt-4 grid grid-flow-col grid-rows-7 gap-1">
        {cells.map((cell, i) => (
          <div
            key={i}
            title={cell.future ? "" : `${cell.date}: ${cell.words} words`}
            className={
              "h-3.5 w-3.5 rounded-[3px] " +
              (cell.future ? "opacity-0" : fill[level(cell.words)])
            }
          />
        ))}
      </div>
      <div className="mt-3 flex items-center justify-end gap-1.5 text-[10px] text-ink-faint">
        <span>Less</span>
        {fill.map((f, i) => (
          <div key={i} className={"h-3 w-3 rounded-[3px] " + f} />
        ))}
        <span>More</span>
      </div>
    </section>
  );
}

/** "Your Voice" — a small profile unlocked after enough all-time words. */
function VoiceProfileCard({ stats }: { stats: Stats | null }) {
  const words = stats?.totalWords ?? 0;
  const unlocked = words >= VOICE_PROFILE_WORDS;

  if (!unlocked) {
    const pct = Math.min(100, (words / VOICE_PROFILE_WORDS) * 100);
    return (
      <section className="mt-4 rounded-2xl border border-dashed border-border bg-surface/50 p-5">
        <div className="flex items-center gap-1.5 text-xs text-ink-faint">
          <Lock size={14} /> Your Voice — locked
        </div>
        <p className="mt-2 text-sm text-ink-soft">
          Dictate {VOICE_PROFILE_WORDS.toLocaleString()} words to unlock your voice
          profile. {(VOICE_PROFILE_WORDS - words).toLocaleString()} to go.
        </p>
        <div className="mt-3 h-2 overflow-hidden rounded-full bg-surface-2">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-500"
            style={{ width: `${pct}%` }}
          />
        </div>
      </section>
    );
  }

  const avgWords = stats!.totalSessions > 0 ? words / stats!.totalSessions : 0;
  const avgSec = stats!.totalSessions > 0 ? stats!.totalMs / stats!.totalSessions / 1000 : 0;
  const top = [...stats!.appUsage].sort((a, b) => b.sessions - a.sessions)[0];

  return (
    <section className="mt-4 rounded-2xl border border-accent/40 bg-accent-soft/40 p-5">
      <div className="flex items-center gap-1.5 text-xs text-ink-faint">
        <Sparkles size={14} className="text-accent" /> Your Voice
      </div>
      <div className="mt-4 grid grid-cols-3 gap-4">
        <ProfileStat label="Words spoken" value={words.toLocaleString()} />
        <ProfileStat label="Words / dictation" value={String(Math.round(avgWords))} />
        <ProfileStat label="Avg length" value={`${avgSec.toFixed(1)}s`} />
      </div>
      {top && (
        <p className="mt-4 text-sm text-ink-soft">
          You dictate most in{" "}
          <span className="font-medium text-ink">{categoryLabel(top.category)}</span>.
        </p>
      )}
    </section>
  );
}

function ProfileStat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="font-serif text-2xl tabular-nums">{value}</div>
      <div className="mt-0.5 text-xs text-ink-faint">{label}</div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="mt-8 rounded-2xl border border-dashed border-border bg-surface/50 p-10 text-center">
      <p className="text-ink-soft">No dictations in this window yet.</p>
      <p className="mt-1 text-sm text-ink-faint">
        Hold your hotkey, speak, and release — your speed, cleanup, and streak show up
        here.
      </p>
    </div>
  );
}
