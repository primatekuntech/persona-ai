import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface AuditEntry {
  id: string;
  actor_id: string;
  actor_email: string | null;
  action: string;
  target_kind: string | null;
  target_id: string | null;
  created_at: string;
}

interface AuditResponse {
  items: AuditEntry[];
  next_cursor: string | null;
}

export default function AdminAudit() {
  const [actor, setActor] = useState("");
  const [action, setAction] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [cursor, setCursor] = useState<string | null>(null);

  function buildQuery() {
    const params = new URLSearchParams({ limit: "50" });
    if (actor) params.set("actor", actor);
    if (action) params.set("action", action);
    if (dateFrom) params.set("from", dateFrom);
    if (dateTo) params.set("to", dateTo);
    if (cursor) params.set("cursor", cursor);
    return `/api/admin/audit?${params.toString()}`;
  }

  const { data, isLoading, isError } = useQuery<AuditResponse, ApiError>({
    queryKey: ["admin", "audit", actor, action, dateFrom, dateTo, cursor],
    queryFn: () => api<AuditResponse>(buildQuery()),
  });

  function applyFilters(e: React.FormEvent) {
    e.preventDefault();
    setCursor(null);
  }

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Audit log</h1>
      </div>

      <div className="flex-1 overflow-auto p-6 space-y-6">
        {/* Filters */}
        <form
          onSubmit={applyFilters}
          className="flex flex-wrap gap-4 items-end p-4 rounded-lg border border-[var(--border)] bg-[var(--bg-subtle)]"
        >
          <div className="space-y-1.5 min-w-40">
            <Label htmlFor="audit-actor">Actor (email)</Label>
            <Input
              id="audit-actor"
              type="text"
              placeholder="user@example.com"
              value={actor}
              onChange={(e) => setActor(e.target.value)}
            />
          </div>
          <div className="space-y-1.5 min-w-40">
            <Label htmlFor="audit-action">Action</Label>
            <Input
              id="audit-action"
              type="text"
              placeholder="e.g. login, delete"
              value={action}
              onChange={(e) => setAction(e.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="audit-from">From</Label>
            <Input
              id="audit-from"
              type="date"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="audit-to">To</Label>
            <Input
              id="audit-to"
              type="date"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
            />
          </div>
          <Button type="submit" variant="outline">
            Apply filters
          </Button>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              setActor("");
              setAction("");
              setDateFrom("");
              setDateTo("");
              setCursor(null);
            }}
          >
            Clear
          </Button>
        </form>

        {/* Table */}
        {isLoading && (
          <p className="text-sm text-[var(--text-muted)]">Loading…</p>
        )}
        {isError && (
          <p className="text-sm text-[var(--danger)]">Failed to load audit log.</p>
        )}
        {!isLoading && !isError && (
          <>
            <div className="rounded-lg border border-[var(--border)] overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Time</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Actor</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Action</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Target</th>
                  </tr>
                </thead>
                <tbody>
                  {data?.items.map((entry) => (
                    <tr
                      key={entry.id}
                      className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                    >
                      <td className="px-4 py-3 text-xs text-[var(--text-subtle)] whitespace-nowrap">
                        {new Date(entry.created_at).toLocaleString()}
                      </td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">
                        {entry.actor_email ?? entry.actor_id}
                      </td>
                      <td className="px-4 py-3 font-mono text-xs text-[var(--text)]">
                        {entry.action}
                      </td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">
                        {entry.target_kind ? (
                          <span>
                            {entry.target_kind}
                            {entry.target_id ? (
                              <span className="ml-1 text-xs text-[var(--text-subtle)]">
                                {entry.target_id}
                              </span>
                            ) : null}
                          </span>
                        ) : (
                          "—"
                        )}
                      </td>
                    </tr>
                  ))}
                  {data?.items.length === 0 && (
                    <tr>
                      <td colSpan={4} className="px-4 py-8 text-center text-[var(--text-muted)]">
                        No audit entries found.
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
