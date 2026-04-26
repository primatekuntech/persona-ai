import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";

interface ErrorEntry {
  id: string;
  route: string;
  code: string;
  message: string;
  created_at: string;
}

interface ErrorsResponse {
  items: ErrorEntry[];
  next_cursor: string | null;
}

export default function AdminErrors() {
  const [cursor, setCursor] = useState<string | null>(null);

  const { data, isLoading, isError } = useQuery<ErrorsResponse, ApiError>({
    queryKey: ["admin", "errors", cursor],
    queryFn: () =>
      api<ErrorsResponse>(
        `/api/admin/errors?limit=50${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
      ),
  });

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Error log</h1>
      </div>

      <div className="flex-1 overflow-auto p-6">
        {isLoading && (
          <p className="text-sm text-[var(--text-muted)]">Loading…</p>
        )}
        {isError && (
          <p className="text-sm text-[var(--danger)]">Failed to load error log.</p>
        )}
        {!isLoading && !isError && (
          <>
            <div className="rounded-lg border border-[var(--border)] overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Route</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Code</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Message</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Time</th>
                  </tr>
                </thead>
                <tbody>
                  {data?.items.map((err) => (
                    <tr
                      key={err.id}
                      className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                    >
                      <td className="px-4 py-3 font-mono text-xs text-[var(--text-muted)]">
                        {err.route}
                      </td>
                      <td className="px-4 py-3 font-mono text-xs text-[var(--danger)]">
                        {err.code}
                      </td>
                      <td className="px-4 py-3 text-[var(--text)] max-w-sm truncate" title={err.message}>
                        {err.message}
                      </td>
                      <td className="px-4 py-3 text-xs text-[var(--text-subtle)]">
                        {new Date(err.created_at).toLocaleString()}
                      </td>
                    </tr>
                  ))}
                  {data?.items.length === 0 && (
                    <tr>
                      <td colSpan={4} className="px-4 py-8 text-center text-[var(--text-muted)]">
                        No errors recorded.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>

            {/* Pagination */}
            <div className="flex items-center justify-between mt-4">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setCursor(null)}
                disabled={cursor === null}
              >
                First page
              </Button>
              {data?.next_cursor && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setCursor(data.next_cursor)}
                >
                  Next page
                </Button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
