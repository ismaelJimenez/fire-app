import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { accountSelectOptions } from "../accounts";
import { formatMoney } from "../format";
import type { ImportResult, View } from "../types";

const TEMPLATE = `date,amount,description,category
2026-01-05,-42.90,Grocery store,Groceries
2026-01-06,1500.00,Monthly salary,Income
2026-01-07,-12.00,Coffee shop,Dining
2026-01-08,-800.00,Transfer to savings,`;

interface Props {
  accountId: number | null;
  onNavigate: (view: View, accountId?: number | null) => void;
}

/** Decode an uploaded file, tolerating the ISO-8859-1 / windows-1252 encoding many
 *  German bank exports (e.g. ING-DiBa) use — reading those as UTF-8 mangles umlauts. */
async function decodeFile(file: File): Promise<string> {
  const buf = await file.arrayBuffer();
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(buf);
  } catch {
    return new TextDecoder("windows-1252").decode(buf);
  }
}

export function Import({ accountId, onNavigate }: Props) {
  const { accounts, refreshAll, toast } = useStore();
  const [target, setTarget] = useState<number | null>(
    accountId ?? accounts[0]?.id ?? null,
  );
  const [csv, setCsv] = useState("");
  const [fileName, setFileName] = useState<string | null>(null);
  const [drag, setDrag] = useState(false);
  const [busy, setBusy] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [preview, setPreview] = useState<ImportResult | null>(null);
  const [detected, setDetected] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  // Surface the auto-detected bank format as the CSV changes, so a silent
  // misdetection (an unrecognized export falling back to the canonical template)
  // is visible before the user imports. Debounced so typing/pasting isn't chatty.
  useEffect(() => {
    if (!csv.trim()) {
      setDetected(null);
      return;
    }
    const id = setTimeout(() => {
      api
        .detectBank(csv)
        .then(setDetected)
        .catch(() => setDetected(null));
    }, 250);
    return () => clearTimeout(id);
  }, [csv]);

  async function loadFile(file: File) {
    const text = await decodeFile(file);
    setCsv(text);
    setFileName(file.name);
    setResult(null);
    setPreview(null);
  }

  function downloadTemplate() {
    const blob = new Blob([TEMPLATE], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "fire-template.csv";
    a.click();
    URL.revokeObjectURL(url);
  }

  async function runPreview() {
    if (target == null) {
      toast("Choose an account to import into", "error");
      return;
    }
    if (!csv.trim()) {
      toast("Load or paste a CSV first", "error");
      return;
    }
    setPreviewing(true);
    setResult(null);
    try {
      const res = await api.importCsv(target, csv, true);
      setPreview(res);
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setPreviewing(false);
    }
  }

  async function runImport() {
    if (target == null) {
      toast("Choose an account to import into", "error");
      return;
    }
    if (!csv.trim()) {
      toast("Load or paste a CSV first", "error");
      return;
    }
    setBusy(true);
    setResult(null);
    setPreview(null);
    try {
      const res = await api.importCsv(target, csv);
      setResult(res);
      await refreshAll();
      if (res.imported > 0) {
        toast(
          `Imported ${res.imported} transaction(s)` +
            (res.skipped_duplicates
              ? `, skipped ${res.skipped_duplicates} duplicate(s)`
              : ""),
        );
      } else if (res.errors.length === 0) {
        toast("Nothing new to import", "success");
      }
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setBusy(false);
    }
  }

  const lineCount = csv.trim() ? csv.trim().split(/\r?\n/).length : 0;
  const options = accountSelectOptions(accounts);
  const targetPath = options.find((a) => a.id === target)?.path ?? null;

  return (
    <div>
      <div className="page-head">
        <div>
          <h1>Import CSV</h1>
          <p>Bring in transactions from your bank export.</p>
        </div>
      </div>

      {accounts.length === 0 ? (
        <div className="empty card">
          <div className="big">🏦</div>
          <h3>Create an account first</h3>
          <p>
            You need at least one account before importing. Add one from the
            sidebar.
          </p>
        </div>
      ) : (
        <div
          style={{ display: "grid", gridTemplateColumns: "1.3fr 1fr", gap: 20 }}
        >
          <div className="card" style={{ padding: 20 }}>
            <div className="field">
              <label>Import into account</label>
              <select
                value={target ?? ""}
                onChange={(e) => setTarget(Number(e.target.value))}
              >
                {options.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.path}
                  </option>
                ))}
              </select>
            </div>

            <div
              className={"drop-zone" + (drag ? " drag" : "")}
              onClick={() => fileInput.current?.click()}
              onDragOver={(e) => {
                e.preventDefault();
                setDrag(true);
              }}
              onDragLeave={() => setDrag(false)}
              onDrop={(e) => {
                e.preventDefault();
                setDrag(false);
                const f = e.dataTransfer.files[0];
                if (f) loadFile(f);
              }}
            >
              <div style={{ fontSize: 28, marginBottom: 6 }}>📄</div>
              {fileName ? (
                <div>
                  <strong>{fileName}</strong>
                  <div className="muted">{lineCount} line(s) loaded</div>
                </div>
              ) : csv.trim() ? (
                <div>
                  <strong>Pasted CSV</strong>
                  <div className="muted">{lineCount} line(s)</div>
                </div>
              ) : (
                <div>
                  <strong>Drop a .csv file here</strong>
                  <div className="muted">or click to browse</div>
                </div>
              )}
              <input
                ref={fileInput}
                type="file"
                accept=".csv,text/csv"
                hidden
                onChange={(e) => {
                  const f = e.target.files?.[0];
                  if (f) loadFile(f);
                  e.target.value = "";
                }}
              />
            </div>

            {detected && (
              <div className="muted" style={{ marginTop: 10, fontSize: 13 }}>
                Detected format: <strong>{detected}</strong>
                {detected === "Canonical template" && (
                  <span>
                    {" "}
                    — if this is a bank export, it wasn’t recognized and will be
                    read as the plain template.
                  </span>
                )}
              </div>
            )}

            <div className="field" style={{ marginTop: 16 }}>
              <label>…or paste CSV text</label>
              <textarea
                rows={6}
                className="mono"
                placeholder={TEMPLATE}
                value={csv}
                onChange={(e) => {
                  setCsv(e.target.value);
                  setFileName(null);
                  setResult(null);
                  setPreview(null);
                }}
              />
            </div>

            <div style={{ display: "flex", gap: 10, marginTop: 4 }}>
              <button
                className="btn primary"
                onClick={runPreview}
                disabled={previewing || busy || !csv.trim()}
              >
                {previewing ? "Checking…" : "Preview changes"}
              </button>
              {csv && (
                <button
                  className="btn"
                  onClick={() => {
                    setCsv("");
                    setFileName(null);
                    setResult(null);
                    setPreview(null);
                  }}
                >
                  Clear
                </button>
              )}
            </div>

            {preview && (
              <div
                className="card"
                style={{
                  marginTop: 18,
                  padding: 16,
                  background: "var(--surface-2)",
                }}
              >
                <div style={{ fontWeight: 600, marginBottom: 8 }}>
                  Dry run — nothing imported yet
                </div>
                <div>
                  ✅ Would import: <strong>{preview.imported}</strong>
                  {targetPath && (
                    <>
                      {" "}
                      into <strong>{targetPath}</strong>
                    </>
                  )}
                </div>
                <div className="muted">
                  ⏭ Would skip as duplicates: {preview.skipped_duplicates}
                </div>
                {(() => {
                  const newCats = [
                    ...new Set(
                      preview.preview
                        .filter((r) => r.new_category && r.category)
                        .map((r) => r.category as string),
                    ),
                  ];
                  return newCats.length > 0 ? (
                    <div className="muted">
                      🏷 Would create categor
                      {newCats.length === 1 ? "y" : "ies"}: {newCats.join(", ")}
                    </div>
                  ) : null;
                })()}
                {preview.errors.length > 0 && (
                  <details style={{ marginTop: 8 }}>
                    <summary style={{ color: "var(--negative)" }}>
                      {preview.errors.length} row(s) had problems
                    </summary>
                    <div className="code-block" style={{ marginTop: 8 }}>
                      {preview.errors.join("\n")}
                    </div>
                  </details>
                )}

                {preview.preview.length > 0 && (
                  <div className="table-wrap" style={{ marginTop: 12 }}>
                    <table style={{ fontSize: 13 }}>
                      <thead>
                        <tr>
                          <th>Date</th>
                          <th>Description</th>
                          <th style={{ textAlign: "right" }}>Amount</th>
                          <th>Category</th>
                          <th>Status</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.preview.map((r, i) => (
                          <tr
                            key={i}
                            style={{ opacity: r.duplicate ? 0.5 : 1 }}
                          >
                            <td className="mono">{r.date}</td>
                            <td>{r.counterparty || r.description || "—"}</td>
                            <td
                              className="mono"
                              style={{
                                textAlign: "right",
                                color:
                                  r.amount < 0
                                    ? "var(--negative)"
                                    : "var(--positive)",
                              }}
                            >
                              {formatMoney(r.amount)}
                            </td>
                            <td>
                              {r.category ? (
                                <>
                                  {r.category}
                                  {r.new_category && (
                                    <span className="muted"> (new)</span>
                                  )}
                                  {r.auto_classified && (
                                    <span className="muted"> · auto</span>
                                  )}
                                </>
                              ) : (
                                <span className="muted">—</span>
                              )}
                            </td>
                            <td>
                              {r.duplicate ? (
                                <span className="muted">duplicate</span>
                              ) : (
                                <span style={{ color: "var(--positive)" }}>
                                  new
                                </span>
                              )}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}

                {preview.imported > 0 && (
                  <div style={{ marginTop: 12 }}>
                    <div className="muted" style={{ marginBottom: 8 }}>
                      Confirm to add {preview.imported} transaction(s)
                      {targetPath ? (
                        <>
                          {" "}
                          to <strong>{targetPath}</strong>
                        </>
                      ) : null}
                      .
                    </div>
                    <button
                      className="btn primary small"
                      onClick={runImport}
                      disabled={busy}
                    >
                      {busy
                        ? "Importing…"
                        : `Confirm import into ${targetPath ?? "account"}`}
                    </button>
                  </div>
                )}
              </div>
            )}

            {result && (
              <div
                className="card"
                style={{
                  marginTop: 18,
                  padding: 16,
                  background: "var(--surface-2)",
                }}
              >
                <div style={{ fontWeight: 600, marginBottom: 8 }}>
                  Import summary
                </div>
                <div>
                  ✅ Imported: <strong>{result.imported}</strong>
                </div>
                <div className="muted">
                  ⏭ Skipped duplicates: {result.skipped_duplicates}
                </div>
                {result.errors.length > 0 && (
                  <details style={{ marginTop: 8 }}>
                    <summary style={{ color: "var(--negative)" }}>
                      {result.errors.length} row(s) had problems
                    </summary>
                    <div className="code-block" style={{ marginTop: 8 }}>
                      {result.errors.join("\n")}
                    </div>
                  </details>
                )}
                {result.imported > 0 && (
                  <button
                    className="btn small"
                    style={{ marginTop: 12 }}
                    onClick={() => onNavigate("transactions", target)}
                  >
                    View imported transactions →
                  </button>
                )}
              </div>
            )}
          </div>

          <div className="card" style={{ padding: 20 }}>
            <div className="section-title" style={{ marginTop: 0 }}>
              Expected format
            </div>
            <p className="muted" style={{ marginTop: 0 }}>
              A header row followed by one transaction per line:
            </p>
            <div className="code-block">{TEMPLATE}</div>
            <ul style={{ paddingLeft: 18, color: "var(--text-muted)" }}>
              <li>
                <strong>date</strong> — <span className="mono">YYYY-MM-DD</span>{" "}
                (also accepts DD/MM/YYYY)
              </li>
              <li>
                <strong>amount</strong> — decimal; negative is an expense,
                positive is income
              </li>
              <li>
                <strong>description</strong> — free text
              </li>
              <li>
                <strong>category</strong> — optional; created automatically if
                new
              </li>
            </ul>
            <p className="muted" style={{ fontSize: 12.5 }}>
              Columns are matched by header name, so their order doesn’t matter.
              Rows identical to ones already imported are skipped automatically.
            </p>
            <p className="muted" style={{ fontSize: 12.5 }}>
              Bank exports are detected automatically — you can drop an{" "}
              <strong>ING-DiBa</strong>, <strong>ING España</strong>,{" "}
              <strong>comdirect</strong>, or <strong>Deutsche Bank</strong> CSV
              here as-is, no reformatting needed. Payees you’ve categorized
              before are classified for you on import.
            </p>
            <button className="btn" onClick={downloadTemplate}>
              ↓ Download template
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
