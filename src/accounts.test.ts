import { describe, it, expect } from "vitest";
import {
  buildAccountTree,
  flattenTree,
  findNode,
  countDescendantAccounts,
  accountSelectOptions,
} from "./accounts";
import type { Account } from "./types";

/** Build an Account with sensible defaults for the fields under test. */
function acc(
  partial: Partial<Account> & { id: number; name: string },
): Account {
  return {
    parent_id: null,
    created_at: "2026-01-01",
    balance: 0,
    tx_count: 0,
    ...partial,
  };
}

// Checking ─ Savings (child) ─ Vacation (grandchild); plus a top-level Cash.
const accounts: Account[] = [
  acc({ id: 1, name: "Checking", balance: 10000, tx_count: 2 }),
  acc({ id: 2, name: "Savings", parent_id: 1, balance: 5000, tx_count: 1 }),
  acc({ id: 3, name: "Vacation", parent_id: 2, balance: 2000, tx_count: 3 }),
  acc({ id: 4, name: "Cash", balance: 500, tx_count: 1 }),
];

describe("buildAccountTree", () => {
  it("nests subaccounts under their parents", () => {
    const tree = buildAccountTree(accounts);
    expect(tree.map((n) => n.account.name)).toEqual(["Cash", "Checking"]);
    const checking = tree.find((n) => n.account.name === "Checking")!;
    expect(checking.children.map((c) => c.account.name)).toEqual(["Savings"]);
    expect(checking.children[0].children[0].account.name).toBe("Vacation");
  });

  it("rolls descendant balances and tx counts up into ancestors", () => {
    const checking = buildAccountTree(accounts).find(
      (n) => n.account.name === "Checking",
    )!;
    // 10000 + 5000 + 2000
    expect(checking.rollupBalance).toBe(17000);
    // 2 + 1 + 3
    expect(checking.rollupTxCount).toBe(6);
  });

  it("assigns depth by nesting level", () => {
    const nodes = flattenTree(buildAccountTree(accounts));
    const depthOf = (name: string) =>
      nodes.find((n) => n.account.name === name)!.depth;
    expect(depthOf("Checking")).toBe(0);
    expect(depthOf("Savings")).toBe(1);
    expect(depthOf("Vacation")).toBe(2);
  });

  it("surfaces orphans (missing parent) as top-level instead of dropping them", () => {
    const orphaned = [acc({ id: 9, name: "Ghost", parent_id: 999 })];
    const tree = buildAccountTree(orphaned);
    expect(tree.map((n) => n.account.name)).toEqual(["Ghost"]);
  });
});

describe("tree helpers", () => {
  it("findNode locates a node anywhere in the tree", () => {
    const tree = buildAccountTree(accounts);
    expect(findNode(tree, 3)?.account.name).toBe("Vacation");
    expect(findNode(tree, 404)).toBeUndefined();
  });

  it("countDescendantAccounts counts every level below a node", () => {
    const checking = buildAccountTree(accounts).find(
      (n) => n.account.name === "Checking",
    )!;
    expect(countDescendantAccounts(checking)).toBe(2); // Savings + Vacation
  });
});

describe("accountSelectOptions", () => {
  it("orders parents before children and indents by depth", () => {
    const opts = accountSelectOptions(accounts);
    expect(opts.map((o) => o.id)).toEqual([4, 1, 2, 3]);
    const vacation = opts.find((o) => o.id === 3)!;
    expect(vacation.depth).toBe(2);
    // Indentation uses non-breaking spaces so it survives <option> rendering.
    expect(vacation.label).toBe("  ".repeat(2) + "↳ Vacation");
  });
  it("spells out the full ancestry in `path` so same-named subaccounts stay distinct", () => {
    const opts = accountSelectOptions(accounts);
    expect(opts.find((o) => o.id === 3)!.path).toBe(
      "Checking › Savings › Vacation",
    );
    expect(opts.find((o) => o.id === 4)!.path).toBe("Cash");
  });
});
