import { useEffect, useMemo, useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import {
  formatMoney,
  formatMoneyCompact,
  formatMonth,
  todayIso,
} from "../format";
import { periodRange, type Period } from "../trends";
import type {
  CategorySpend,
  MonthlyPoint,
  NetWorthPoint,
  View,
} from "../types";

interface Props {
  onNavigate: (view: View, accountId?: number | null) => void;
}

const PERIODS: { id: Period; label: string }[] = [
  { id: "ytd", label: "Year to date" },
  { id: "12m", label: "Last 12 months" },
  { id: "all", label: "All time" },
];

export function Trends({ onNavigate }: Props) {
  const { accounts, toast } = useStore();
  const [period, setPeriod] = useState<Period>("ytd");
  const [monthly, setMonthly] = useState<MonthlyPoint[] | null>(null);
  const [networth, setNetworth] = useState<NetWorthPoint[] | null>(null);
  const [categories, setCategories] = useState<CategorySpend[] | null>(null);
  const [incomeCategories, setIncomeCategories] = useState<
    CategorySpend[] | null
  >(null);

  useEffect(() => {
    const { from, to } = periodRange(period, todayIso());
    let alive = true;
    setMonthly(null);
    setNetworth(null);
    setCategories(null);
    setIncomeCategories(null);
    Promise.all([
      api.monthlySeries(from, to),
      api.networthSeries(from, to),
      api.categoryBreakdown(from, to, "expense"),
      api.categoryBreakdown(from, to, "income"),
    ])
      .then(([m, n, c, ic]) => {
        if (!alive) return;
        setMonthly(m);
        setNetworth(n);
        setCategories(c);
        setIncomeCategories(ic);
      })
      .catch((err) => toast(String(err), "error"));
    return () => {
      alive = false;
    };
  }, [period, toast]);

  const kpis = useMemo(() => {
    if (!monthly || monthly.length === 0) return null;
    const income = monthly.reduce((s, p) => s + p.income, 0);
    const expenses = monthly.reduce((s, p) => s + p.expenses, 0); // negative
    const net = income + expenses;
    const months = monthly.length;
    return {
      income,
      expenses,
      net,
      months,
      meanIncome: Math.round(income / months),
      meanExpense: Math.round(expenses / months),
      meanSaving: Math.round(net / months),
      // Undefined (shown as "—") when there's no income to divide by.
      savingsRate: income > 0 ? net / income : null,
    };
  }, [monthly]);

  if (accounts.length === 0) {
    return (
      <div>
        <PageHead />
        <div className="empty card">
          <div className="big">📈</div>
          <h3>Nothing to chart yet</h3>
          <p>
            Create an account and import or add some transactions, and your
            income, spending, and net worth over time will show up here.
          </p>
          <button className="btn primary" onClick={() => onNavigate("import")}>
            Go to import
          </button>
        </div>
      </div>
    );
  }

  const loading = monthly === null;
  const hasData = !!monthly && monthly.length > 0;

  return (
    <div>
      <div className="page-head">
        <div>
          <h1>Trends</h1>
          <p>How your income, spending, and net worth change over time.</p>
        </div>
        <div className="seg-control" role="tablist" aria-label="Time range">
          {PERIODS.map((p) => (
            <button
              key={p.id}
              role="tab"
              aria-selected={period === p.id}
              className={"seg" + (period === p.id ? " active" : "")}
              onClick={() => setPeriod(p.id)}
            >
              {p.label}
            </button>
          ))}
        </div>
      </div>

      {loading ? (
        <div className="empty card">
          <p className="muted">Loading…</p>
        </div>
      ) : !hasData ? (
        <div className="empty card">
          <div className="big">📈</div>
          <h3>No transactions in this period</h3>
          <p>Try a wider time range, or import some transactions first.</p>
        </div>
      ) : (
        <>
          <div className="stat-grid">
            <Stat label="Income" value={formatMoney(kpis!.income)} tone="pos" />
            <Stat
              label="Expenses"
              value={formatMoney(kpis!.expenses)}
              tone="neg"
            />
            <Stat
              label="Net savings"
              value={formatMoney(kpis!.net)}
              tone={kpis!.net < 0 ? "neg" : "pos"}
            />
            <Stat
              label="Savings rate"
              value={
                kpis!.savingsRate == null
                  ? "—"
                  : `${Math.round(kpis!.savingsRate * 100)}%`
              }
              tone={
                kpis!.savingsRate != null && kpis!.savingsRate < 0
                  ? "neg"
                  : undefined
              }
            />
          </div>

          <div className="stat-grid" style={{ marginTop: -8 }}>
            <Stat
              label={`Mean income / month (${kpis!.months} mo)`}
              value={formatMoney(kpis!.meanIncome)}
              tone="pos"
              small
            />
            <Stat
              label={`Mean expense / month (${kpis!.months} mo)`}
              value={formatMoney(kpis!.meanExpense)}
              tone="neg"
              small
            />
            <Stat
              label={`Mean savings / month (${kpis!.months} mo)`}
              value={formatMoney(kpis!.meanSaving)}
              tone={kpis!.meanSaving < 0 ? "neg" : "pos"}
              small
            />
          </div>

          <div className="section-title">Income vs. expenses</div>
          <div className="card chart-card">
            <IncomeExpenseChart data={monthly!} />
          </div>

          <div className="section-title">Net worth over time</div>
          <div className="card chart-card">
            <NetWorthChart data={networth ?? []} />
          </div>

          <div className="section-title">
            Income by category{" "}
            <span className="muted" style={{ fontWeight: 400, fontSize: 12.5 }}>
              · mean / month
            </span>
          </div>
          <div className="card">
            <CategoryBreakdown
              data={incomeCategories ?? []}
              months={Math.max(1, kpis!.months)}
              emptyText="No income in this period."
              tone="pos"
            />
          </div>

          <div className="section-title">
            Spending by category{" "}
            <span className="muted" style={{ fontWeight: 400, fontSize: 12.5 }}>
              · mean / month
            </span>
          </div>
          <div className="card">
            <CategoryBreakdown
              data={categories ?? []}
              months={Math.max(1, kpis!.months)}
              emptyText="No spending in this period."
              tone="neg"
            />
          </div>
        </>
      )}
    </div>
  );
}

function PageHead() {
  return (
    <div className="page-head">
      <div>
        <h1>Trends</h1>
        <p>How your income, spending, and net worth change over time.</p>
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  tone,
  small,
}: {
  label: string;
  value: string;
  tone?: "pos" | "neg";
  small?: boolean;
}) {
  return (
    <div className="card stat">
      <div className="label">{label}</div>
      <div
        className={"value " + (tone ?? "")}
        style={small ? { fontSize: 20 } : undefined}
      >
        {value}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Charts — inline SVG, using the app's design tokens. A fixed viewBox scales
// responsively; text is in ink tokens, marks carry identity by color.
// ---------------------------------------------------------------------------

const VBW = 760;
const VBH = 260;
const PAD = { top: 16, right: 12, bottom: 30, left: 12 };
const PLOT_W = VBW - PAD.left - PAD.right;
const PLOT_H = VBH - PAD.top - PAD.bottom;

/** Pick ~`max` evenly spaced indices across `n` items for sparse axis labels. */
function labelIndices(n: number, max = 12): Set<number> {
  if (n <= max) return new Set(Array.from({ length: n }, (_, i) => i));
  const step = Math.ceil(n / max);
  const out = new Set<number>();
  for (let i = 0; i < n; i += step) out.add(i);
  out.add(n - 1);
  return out;
}

/** Income up / expenses down, opposed around a shared zero baseline. */
function IncomeExpenseChart({ data }: { data: MonthlyPoint[] }) {
  const [hover, setHover] = useState<number | null>(null);

  const maxUp = Math.max(1, ...data.map((d) => d.income));
  const maxDown = Math.max(1, ...data.map((d) => -d.expenses));
  const total = maxUp + maxDown;
  const baselineY = PAD.top + (maxUp / total) * PLOT_H;
  const upH = baselineY - PAD.top;
  const downH = PAD.top + PLOT_H - baselineY;

  const n = data.length;
  const slot = PLOT_W / n;
  const barW = Math.min(slot * 0.62, 26);
  const cx = (i: number) => PAD.left + slot * (i + 0.5);
  const labels = labelIndices(n);

  return (
    <div className="chart">
      <div className="chart-legend">
        <span className="lg">
          <span className="sw" style={{ background: "var(--positive)" }} />
          Income
        </span>
        <span className="lg">
          <span className="sw" style={{ background: "var(--negative)" }} />
          Expenses
        </span>
      </div>
      <div className="chart-plot">
        <svg
          viewBox={`0 0 ${VBW} ${VBH}`}
          className="chart-svg"
          role="img"
          aria-label="Monthly income versus expenses"
        >
          <line
            x1={PAD.left}
            x2={VBW - PAD.right}
            y1={baselineY}
            y2={baselineY}
            className="axis-line"
          />
          {data.map((d, i) => {
            const ih = maxUp ? (d.income / maxUp) * upH : 0;
            const eh = maxDown ? (-d.expenses / maxDown) * downH : 0;
            const active = hover === i;
            return (
              <g key={d.month} opacity={hover == null || active ? 1 : 0.45}>
                {ih > 0 && (
                  <rect
                    x={cx(i) - barW / 2}
                    y={baselineY - ih}
                    width={barW}
                    height={ih}
                    rx={3}
                    fill="var(--positive)"
                  />
                )}
                {eh > 0 && (
                  <rect
                    x={cx(i) - barW / 2}
                    y={baselineY}
                    width={barW}
                    height={eh}
                    rx={3}
                    fill="var(--negative)"
                  />
                )}
                {/* Full-height hit target for a forgiving hover. */}
                <rect
                  x={PAD.left + slot * i}
                  y={PAD.top}
                  width={slot}
                  height={PLOT_H}
                  fill="transparent"
                  onMouseEnter={() => setHover(i)}
                  onMouseLeave={() => setHover(null)}
                />
              </g>
            );
          })}
          {data.map((d, i) =>
            labels.has(i) ? (
              <text
                key={d.month}
                x={cx(i)}
                y={VBH - 8}
                className="axis-text"
                textAnchor="middle"
              >
                {formatMonth(d.month, true)}
              </text>
            ) : null,
          )}
        </svg>
        <span className="axis-tag top">{formatMoneyCompact(maxUp)}</span>
        <span className="axis-tag bot">{formatMoneyCompact(-maxDown)}</span>
        {hover != null && (
          <ChartTooltip left={cx(hover) / VBW}>
            <strong>{formatMonth(data[hover].month)}</strong>
            <div className="row pos">
              Income <span>{formatMoney(data[hover].income)}</span>
            </div>
            <div className="row neg">
              Expenses <span>{formatMoney(data[hover].expenses)}</span>
            </div>
            <div className="row">
              Net{" "}
              <span>
                {formatMoney(data[hover].income + data[hover].expenses)}
              </span>
            </div>
          </ChartTooltip>
        )}
      </div>
    </div>
  );
}

/** Cumulative net worth as an area + line, with a hover crosshair. */
function NetWorthChart({ data }: { data: NetWorthPoint[] }) {
  const [hover, setHover] = useState<number | null>(null);

  if (data.length === 0) {
    return (
      <div className="empty" style={{ padding: 32 }}>
        No data.
      </div>
    );
  }

  const values = data.map((d) => d.balance);
  const lo = Math.min(0, ...values);
  const hi = Math.max(0, ...values);
  const span = hi - lo || 1;
  const n = data.length;
  const x = (i: number) =>
    n === 1 ? PAD.left + PLOT_W / 2 : PAD.left + (i / (n - 1)) * PLOT_W;
  const y = (v: number) => PAD.top + (1 - (v - lo) / span) * PLOT_H;
  const zeroY = y(0);

  const line = data.map((d, i) => `${x(i)},${y(d.balance)}`).join(" ");
  const area = `${PAD.left},${PAD.top + PLOT_H} ${line} ${PAD.left + PLOT_W},${
    PAD.top + PLOT_H
  }`;
  const labels = labelIndices(n);
  const slot = n > 1 ? PLOT_W / (n - 1) : PLOT_W;

  return (
    <div className="chart">
      <div className="chart-plot">
        <svg
          viewBox={`0 0 ${VBW} ${VBH}`}
          className="chart-svg"
          role="img"
          aria-label="Net worth over time"
        >
          {lo < 0 && (
            <line
              x1={PAD.left}
              x2={VBW - PAD.right}
              y1={zeroY}
              y2={zeroY}
              className="axis-line"
            />
          )}
          <polygon points={area} fill="var(--accent-soft)" opacity={0.6} />
          <polyline
            points={line}
            fill="none"
            stroke="var(--accent)"
            strokeWidth={2}
            strokeLinejoin="round"
            strokeLinecap="round"
          />
          {hover != null && (
            <line
              x1={x(hover)}
              x2={x(hover)}
              y1={PAD.top}
              y2={PAD.top + PLOT_H}
              className="crosshair"
            />
          )}
          {data.map((d, i) => {
            const active = hover === i || n === 1;
            return active ? (
              <circle
                key={d.month}
                cx={x(i)}
                cy={y(d.balance)}
                r={4}
                fill="var(--accent)"
                stroke="var(--surface)"
                strokeWidth={2}
              />
            ) : null;
          })}
          {data.map((d, i) => (
            <rect
              key={d.month}
              x={x(i) - slot / 2}
              y={PAD.top}
              width={slot}
              height={PLOT_H}
              fill="transparent"
              onMouseEnter={() => setHover(i)}
              onMouseLeave={() => setHover(null)}
            />
          ))}
          {data.map((d, i) =>
            labels.has(i) ? (
              <text
                key={d.month}
                x={x(i)}
                y={VBH - 8}
                className="axis-text"
                textAnchor="middle"
              >
                {formatMonth(d.month, true)}
              </text>
            ) : null,
          )}
        </svg>
        <span className="axis-tag top">{formatMoneyCompact(hi)}</span>
        <span className="axis-tag bot">{formatMoneyCompact(lo)}</span>
        {hover != null && (
          <ChartTooltip left={x(hover) / VBW}>
            <strong>{formatMonth(data[hover].month)}</strong>
            <div className="row">
              Net worth <span>{formatMoney(data[hover].balance)}</span>
            </div>
          </ChartTooltip>
        )}
      </div>
    </div>
  );
}

/** Ranked horizontal bars of flow per category. Each row shows the category's
 *  share of the total (bar + %) and its mean per month over the period. Works
 *  for either side of the ledger — expense totals are negative, income totals
 *  positive; the share is a same-signed ratio so it stays positive either way.
 *  Top rows plus an "Other" roll-up. */
function CategoryBreakdown({
  data,
  months,
  emptyText,
  tone,
}: {
  data: CategorySpend[];
  months: number;
  emptyText: string;
  tone: "pos" | "neg";
}) {
  const TOP = 20;
  if (data.length === 0) {
    return (
      <div className="empty" style={{ padding: 32 }}>
        <p className="muted">{emptyText}</p>
      </div>
    );
  }
  const head = data.slice(0, TOP);
  const rest = data.slice(TOP);
  const rows = head.map((d) => ({
    key: d.category_id == null ? "uncat" : String(d.category_id),
    name: d.category_name ?? "Uncategorized",
    total: d.total,
  }));
  if (rest.length > 0) {
    rows.push({
      key: "other",
      name: `Other (${rest.length})`,
      total: rest.reduce((s, d) => s + d.total, 0),
    });
  }
  // A category and the whole carry the same sign, so their ratio is positive
  // whether this is the expense (negative) or income (positive) breakdown.
  const totalSpend = data.reduce((s, d) => s + d.total, 0) || 1;

  return (
    <div className="cat-bars">
      {rows.map((r) => {
        const share = (r.total / totalSpend) * 100;
        return (
          <div className="cat-row" key={r.key}>
            <div className="cat-name" title={r.name}>
              {r.name}
            </div>
            <div className="cat-track">
              <div
                className={`cat-fill ${tone}`}
                style={{ width: `${share}%` }}
              />
            </div>
            <div className="cat-pct">{Math.round(share)}%</div>
            <div className="cat-value">
              {formatMoney(Math.round(r.total / months))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function ChartTooltip({
  left,
  children,
}: {
  left: number; // 0..1 fraction of chart width
  children: React.ReactNode;
}) {
  return (
    <div
      className="chart-tooltip"
      style={{ left: `${Math.min(0.88, Math.max(0.12, left)) * 100}%` }}
    >
      {children}
    </div>
  );
}
