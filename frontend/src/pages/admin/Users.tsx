import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import toast from "react-hot-toast";

interface UserSummary {
  id: string;
  email: string;
  role: string;
  status: string;
  display_name: string | null;
  created_at: string;
  last_login_at: string | null;
}

interface UsersResponse {
  items: UserSummary[];
  next_cursor: string | null;
}

export default function AdminUsers() {
  const qc = useQueryClient();

  const { data, isLoading, isError } = useQuery<UsersResponse, ApiError>({
    queryKey: ["admin", "users"],
    queryFn: () => api<UsersResponse>("/api/admin/users?limit=50"),
  });

  const patchUser = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: string;
      payload: { status?: string; role?: string };
    }) =>
      api(`/api/admin/users/${id}`, {
        method: "PATCH",
        body: JSON.stringify(payload),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "users"] });
      toast.success("User updated.");
    },
    onError: (e: ApiError) => toast.error(e.message),
  });

  const resetUser = useMutation({
    mutationFn: (id: string) =>
      api<{ reset_url: string }>(`/api/admin/users/${id}/reset`, { method: "POST" }),
    onSuccess: (data) => {
      navigator.clipboard.writeText(data.reset_url).then(
        () => toast.success("Reset URL copied to clipboard."),
        () => toast(`Reset URL (copy manually): ${data.reset_url}`, { duration: 10000 }),
      );
    },
    onError: (e: ApiError) => toast.error(e.message),
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-48 text-sm text-[var(--text-muted)]">
        Loading…
      </div>
    );
  }

  if (isError) {
    return (
      <div className="p-6 text-sm text-[var(--danger)]">Failed to load users.</div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Users</h1>
      </div>

      <div className="flex-1 overflow-auto p-6">
        <div className="rounded-lg border border-[var(--border)] overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Email</th>
                <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Name</th>
                <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Role</th>
                <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Status</th>
                <th className="text-right px-4 py-2.5 font-medium text-[var(--text-muted)]">Actions</th>
              </tr>
            </thead>
            <tbody>
              {data?.items.map((user) => (
                <tr
                  key={user.id}
                  className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                >
                  <td className="px-4 py-3 text-[var(--text)]">{user.email}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)]">
                    {user.display_name ?? "—"}
                  </td>
                  <td className="px-4 py-3">
                    <span className="capitalize text-[var(--text-muted)]">{user.role}</span>
                  </td>
                  <td className="px-4 py-3">
                    <span
                      className={
                        user.status === "active"
                          ? "text-[var(--success)]"
                          : "text-[var(--text-subtle)]"
                      }
                    >
                      {user.status}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center justify-end gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() =>
                          patchUser.mutate({
                            id: user.id,
                            payload: {
                              status: user.status === "active" ? "disabled" : "active",
                            },
                          })
                        }
                      >
                        {user.status === "active" ? "Disable" : "Enable"}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() =>
                          patchUser.mutate({
                            id: user.id,
                            payload: {
                              role: user.role === "admin" ? "user" : "admin",
                            },
                          })
                        }
                      >
                        Make {user.role === "admin" ? "user" : "admin"}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => resetUser.mutate(user.id)}
                      >
                        Reset pw
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
              {data?.items.length === 0 && (
                <tr>
                  <td colSpan={5} className="px-4 py-8 text-center text-[var(--text-muted)]">
                    No users yet.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
