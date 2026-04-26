import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import toast from "react-hot-toast";

interface JobStat {
  kind: string;
  status: string;
  count: number;
  oldest_queued_at: string | null;
  longest_running_started_at: string | null;
}

interface Job {
  id: string;
  kind: string;
  status: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  error: string | null;
  attempts: number;
}

interface JobsStatsResponse {
  stats: JobStat[];
}

interface JobsListResponse {
  items: Job[];
  next_cursor: string | null;
}

function fmtDate(s: string | null) {
  if (!s) return "—";
  return new Date(s).toLocaleString();
}

function statusColor(status: string) {
  switch (status) {
    case "done":
      return "text-[var(--success)]";
    case "failed":
      return "text-[var(--danger)]";
    case "running":
      return "text-[var(--warning)]";
    default:
      return "text-[var(--text-muted)]";
  }
}

export default function AdminJobs() {
  const qc = useQueryClient();

  const { data: statsData, isLoading: statsLoading } = useQuery<JobsStatsResponse, ApiError>({
    queryKey: ["admin", "jobs", "stats"],
    queryFn: () => api<JobsStatsResponse>("/api/admin/jobs/stats"),
    refetchInterval: 10000,
  });

  const { data: listData, isLoading: listLoading } = useQuery<JobsListResponse, ApiError>({
    queryKey: ["admin", "jobs", "list"],
    queryFn: () => api<JobsListResponse>("/api/admin/jobs?limit=50"),
    refetchInterval: 10000,
  });

  const retryJob = useMutation({
    mutationFn: (id: string) =>
      api(`/api/admin/jobs/${id}/retry`, { method: "POST" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "jobs"] });
      toast.success("Job queued for retry.");
    },
    onError: (e: ApiError) => toast.error(e.message),
  });

  const cancelJob = useMutation({
    mutationFn: (id: string) =>
      api(`/api/admin/jobs/${id}/cancel`, { method: "POST" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "jobs"] });
      toast.success("Job cancelled.");
    },
    onError: (e: ApiError) => toast.error(e.message),
  });

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Job queue</h1>
      </div>

      <div className="flex-1 overflow-auto p-6 space-y-6">
        {/* Stats table */}
        <div>
          <h2 className="text-sm font-medium text-[var(--text-muted)] mb-3">
            Queue summary <span className="font-normal">(refreshes every 10s)</span>
          </h2>
          {statsLoading ? (
            <p className="text-sm text-[var(--text-muted)]">Loading…</p>
          ) : (
            <div className="rounded-lg border border-[var(--border)] overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Kind</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Status</th>
                    <th className="text-right px-4 py-2.5 font-medium text-[var(--text-muted)]">Count</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Oldest queued</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Longest running</th>
                  </tr>
                </thead>
                <tbody>
                  {statsData?.stats.map((row, i) => (
                    <tr
                      key={i}
                      className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                    >
                      <td className="px-4 py-3 text-[var(--text)] font-mono text-xs">{row.kind}</td>
                      <td className={`px-4 py-3 ${statusColor(row.status)}`}>{row.status}</td>
                      <td className="px-4 py-3 text-right text-[var(--text)]">{row.count}</td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">
                        {fmtDate(row.oldest_queued_at)}
                      </td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">
                        {fmtDate(row.longest_running_started_at)}
                      </td>
                    </tr>
                  ))}
                  {(statsData?.stats.length ?? 0) === 0 && (
                    <tr>
                      <td colSpan={5} className="px-4 py-8 text-center text-[var(--text-muted)]">
                        No jobs.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Recent jobs list */}
        <div>
          <h2 className="text-sm font-medium text-[var(--text-muted)] mb-3">Recent jobs</h2>
          {listLoading ? (
            <p className="text-sm text-[var(--text-muted)]">Loading…</p>
          ) : (
            <div className="rounded-lg border border-[var(--border)] overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Kind</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Status</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Created</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Finished</th>
                    <th className="text-right px-4 py-2.5 font-medium text-[var(--text-muted)]">Attempts</th>
                    <th className="text-right px-4 py-2.5 font-medium text-[var(--text-muted)]">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {listData?.items.map((job) => (
                    <tr
                      key={job.id}
                      className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                    >
                      <td className="px-4 py-3 text-[var(--text)] font-mono text-xs">{job.kind}</td>
                      <td className={`px-4 py-3 ${statusColor(job.status)}`}>{job.status}</td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">{fmtDate(job.created_at)}</td>
                      <td className="px-4 py-3 text-[var(--text-muted)]">{fmtDate(job.finished_at)}</td>
                      <td className="px-4 py-3 text-right text-[var(--text-muted)]">{job.attempts}</td>
                      <td className="px-4 py-3">
                        <div className="flex items-center justify-end gap-2">
                          {(job.status === "failed" || job.status === "queued") && (
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => retryJob.mutate(job.id)}
                              disabled={retryJob.isPending}
                            >
                              Retry
                            </Button>
                          )}
                          {(job.status === "queued" || job.status === "running") && (
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => cancelJob.mutate(job.id)}
                              disabled={cancelJob.isPending}
                            >
                              Cancel
                            </Button>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                  {(listData?.items.length ?? 0) === 0 && (
                    <tr>
                      <td colSpan={6} className="px-4 py-8 text-center text-[var(--text-muted)]">
                        No recent jobs.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
