import { useState } from "react";
import { StoreProvider, useStore } from "./store";
import { Sidebar } from "./components/Sidebar";
import { Dashboard } from "./components/Dashboard";
import { Transactions } from "./components/Transactions";
import { Import } from "./components/Import";
import type { View } from "./types";
import "./styles.css";

function Toasts() {
  const { toasts, dismissToast } = useStore();
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={"toast " + t.kind}
          onClick={() => dismissToast(t.id)}
        >
          <span>{t.kind === "success" ? "✅" : "⚠️"}</span>
          <span>{t.message}</span>
        </div>
      ))}
    </div>
  );
}

function Shell() {
  const [view, setView] = useState<View>("dashboard");
  const [accountId, setAccountId] = useState<number | null>(null);

  function navigate(next: View, acc?: number | null) {
    setView(next);
    if (acc !== undefined) setAccountId(acc);
  }

  return (
    <div className="app">
      <Sidebar
        view={view}
        onNavigate={navigate}
        selectedAccountId={accountId}
      />
      <main className="main">
        {view === "dashboard" && <Dashboard onNavigate={navigate} />}
        {view === "transactions" && (
          <Transactions
            accountId={accountId}
            onSelectAccount={(id) => setAccountId(id)}
          />
        )}
        {view === "import" && (
          <Import accountId={accountId} onNavigate={navigate} />
        )}
      </main>
      <Toasts />
    </div>
  );
}

export default function App() {
  return (
    <StoreProvider>
      <Shell />
    </StoreProvider>
  );
}
