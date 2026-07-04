import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorBoundary } from "./ErrorBoundary";

function Boom({ message }: { message: string }): React.ReactElement {
  throw new Error(message);
}

// Throws on first render; flip the flag (as the fix that "Try again" retries
// would) and it renders cleanly on the next attempt.
let shouldThrow = true;
function MaybeThrow() {
  if (shouldThrow) throw new Error("first render fails");
  return <div>recovered content</div>;
}

beforeEach(() => {
  shouldThrow = true;
  // React logs caught errors to console.error; silence it for clean output.
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

describe("ErrorBoundary", () => {
  it("renders children when nothing throws", () => {
    render(
      <ErrorBoundary>
        <div>all good</div>
      </ErrorBoundary>,
    );
    expect(screen.getByText("all good")).toBeInTheDocument();
  });

  it("shows a fallback with the error message when a child throws", () => {
    render(
      <ErrorBoundary>
        <Boom message="kaboom" />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("kaboom")).toBeInTheDocument();
  });

  it("recovers when the user clicks Try again", async () => {
    render(
      <ErrorBoundary>
        <MaybeThrow />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();

    // Simulate the underlying problem being resolved before the retry.
    shouldThrow = false;
    await userEvent.click(screen.getByRole("button", { name: /try again/i }));

    expect(await screen.findByText("recovered content")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
