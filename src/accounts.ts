// Helpers for the account hierarchy. An account may hold subaccounts to any
// depth. The backend reports each account's own balance/tx_count; here we build
// the tree and roll every account's descendants up into it.

import type { Account } from "./types";

export interface AccountNode {
  account: Account;
  children: AccountNode[];
  /** 0 for a top-level account, +1 per level of nesting. */
  depth: number;
  /** Own balance plus every descendant's balance, in cents. */
  rollupBalance: number;
  /** Own transaction count plus every descendant's, for delete warnings. */
  rollupTxCount: number;
}

const collate = (x: Account, y: Account) =>
  x.name.localeCompare(y.name, undefined, { sensitivity: "base" });

/**
 * Build the nested account tree. Children are sorted by name; orphaned accounts
 * (a missing parent) are defensively surfaced as top-level so nothing silently
 * disappears.
 */
export function buildAccountTree(accounts: Account[]): AccountNode[] {
  const ids = new Set(accounts.map((a) => a.id));
  const childrenOf = new Map<number, Account[]>();
  const roots: Account[] = [];

  for (const a of accounts) {
    if (a.parent_id != null && ids.has(a.parent_id)) {
      const list = childrenOf.get(a.parent_id) ?? [];
      list.push(a);
      childrenOf.set(a.parent_id, list);
    } else {
      roots.push(a);
    }
  }

  const build = (account: Account, depth: number): AccountNode => {
    const children = (childrenOf.get(account.id) ?? [])
      .sort(collate)
      .map((c) => build(c, depth + 1));
    const rollupBalance =
      account.balance + children.reduce((s, c) => s + c.rollupBalance, 0);
    const rollupTxCount =
      account.tx_count + children.reduce((s, c) => s + c.rollupTxCount, 0);
    return { account, children, depth, rollupBalance, rollupTxCount };
  };

  return roots.sort(collate).map((r) => build(r, 0));
}

/** Depth-first walk of the tree, parents before their children. */
export function flattenTree(nodes: AccountNode[]): AccountNode[] {
  return nodes.flatMap((n) => [n, ...flattenTree(n.children)]);
}

/** Locate a node anywhere in the tree by account id. */
export function findNode(
  nodes: AccountNode[],
  id: number,
): AccountNode | undefined {
  for (const n of nodes) {
    if (n.account.id === id) return n;
    const found = findNode(n.children, id);
    if (found) return found;
  }
  return undefined;
}

/** Total number of subaccounts beneath a node, at every level. */
export function countDescendantAccounts(node: AccountNode): number {
  return node.children.reduce((s, c) => s + 1 + countDescendantAccounts(c), 0);
}

export interface AccountOption {
  id: number;
  /** Display label; subaccounts are indented per depth to read as nested. */
  label: string;
  depth: number;
}

/**
 * Flatten the tree into `<select>` options, each account immediately followed by
 * its subaccounts, indented by depth so the dropdown mirrors the sidebar order.
 */
export function accountSelectOptions(accounts: Account[]): AccountOption[] {
  return flattenTree(buildAccountTree(accounts)).map((n) => ({
    id: n.account.id,
    label: "  ".repeat(n.depth) + (n.depth > 0 ? "↳ " : "") + n.account.name,
    depth: n.depth,
  }));
}
