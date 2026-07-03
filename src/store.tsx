import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import * as api from "./api";
import type { Account, Category, Summary } from "./types";

interface Toast {
  id: number;
  message: string;
  kind: "success" | "error";
}

interface Store {
  accounts: Account[];
  categories: Category[];
  summary: Summary | null;
  loading: boolean;
  refreshAccounts: () => Promise<void>;
  refreshCategories: () => Promise<void>;
  refreshSummary: () => Promise<void>;
  refreshAll: () => Promise<void>;
  toast: (message: string, kind?: Toast["kind"]) => void;
  toasts: Toast[];
  dismissToast: (id: number) => void;
}

const StoreContext = createContext<Store | null>(null);

let toastId = 0;

export function StoreProvider({ children }: { children: ReactNode }) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [loading, setLoading] = useState(true);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const dismissToast = useCallback((id: number) => {
    setToasts((ts) => ts.filter((t) => t.id !== id));
  }, []);

  const toast = useCallback(
    (message: string, kind: Toast["kind"] = "success") => {
      const id = ++toastId;
      setToasts((ts) => [...ts, { id, message, kind }]);
      setTimeout(() => dismissToast(id), 4000);
    },
    [dismissToast],
  );

  const refreshAccounts = useCallback(async () => {
    setAccounts(await api.listAccounts());
  }, []);
  const refreshCategories = useCallback(async () => {
    setCategories(await api.listCategories());
  }, []);
  const refreshSummary = useCallback(async () => {
    setSummary(await api.getSummary());
  }, []);

  const refreshAll = useCallback(async () => {
    await Promise.all([
      refreshAccounts(),
      refreshCategories(),
      refreshSummary(),
    ]);
  }, [refreshAccounts, refreshCategories, refreshSummary]);

  useEffect(() => {
    refreshAll()
      .catch((err) => toast(String(err), "error"))
      .finally(() => setLoading(false));
  }, [refreshAll, toast]);

  const value = useMemo<Store>(
    () => ({
      accounts,
      categories,
      summary,
      loading,
      refreshAccounts,
      refreshCategories,
      refreshSummary,
      refreshAll,
      toast,
      toasts,
      dismissToast,
    }),
    [
      accounts,
      categories,
      summary,
      loading,
      refreshAccounts,
      refreshCategories,
      refreshSummary,
      refreshAll,
      toast,
      toasts,
      dismissToast,
    ],
  );

  return (
    <StoreContext.Provider value={value}>{children}</StoreContext.Provider>
  );
}

export function useStore(): Store {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore must be used within StoreProvider");
  return ctx;
}
