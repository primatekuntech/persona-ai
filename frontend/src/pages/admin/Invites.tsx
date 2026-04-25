import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import toast from "react-hot-toast";

interface InviteToken {
  token_hash: string;
  email: string;
  role: string;
  expires_at: string;
  used_at: string | null;
  created_at: string;
}

interface InvitesResponse {
  items: InviteToken[];
  next_cursor: string | null;
}

interface CreateInviteResponse {
  invite_url: string;
  token_hash: string;
  expires_at: string;
}

const schema = z.object({
  email: z.string().email("Enter a valid email address."),
  role: z.enum(["user", "admin"]),
});

type FormData = z.infer<typeof schema>;

export default function AdminInvites() {
  const qc = useQueryClient();

  const { data, isLoading } = useQuery<InvitesResponse, ApiError>({
    queryKey: ["admin", "invites"],
    queryFn: () => api<InvitesResponse>("/api/admin/invites?limit=50"),
  });

  const createInvite = useMutation({
    mutationFn: (body: FormData) =>
      api<CreateInviteResponse>("/api/admin/invites", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    onSuccess: (res) => {
      qc.invalidateQueries({ queryKey: ["admin", "invites"] });
      navigator.clipboard.writeText(res.invite_url).then(
        () => toast.success("Invite created. URL copied to clipboard."),
        () => toast(`Invite created. Copy URL manually: ${res.invite_url}`, { duration: 15000 }),
      );
      reset();
    },
    onError: (e: ApiError) => {
      if (e.code === "invite_pending") {
        toast.error("An active invite for this email already exists.");
      } else if (e.code === "user_exists") {
        toast.error("A user with this email already exists.");
      } else {
        toast.error(e.message);
      }
    },
  });

  const revokeInvite = useMutation({
    mutationFn: (tokenHash: string) =>
      api(`/api/admin/invites/${encodeURIComponent(tokenHash)}`, { method: "DELETE" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "invites"] });
      toast.success("Invite revoked.");
    },
    onError: (e: ApiError) => toast.error(e.message),
  });

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<FormData>({
    resolver: zodResolver(schema),
    defaultValues: { role: "user" },
  });

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Invites</h1>
      </div>

      <div className="flex-1 overflow-auto p-6 space-y-6 max-w-3xl">
        {/* Create invite */}
        <Card>
          <form onSubmit={handleSubmit((data) => createInvite.mutate(data))}>
            <CardHeader>
              <CardTitle>Create invite</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex gap-4">
                <div className="flex-1 space-y-1.5">
                  <Label htmlFor="email">Email</Label>
                  <Input
                    id="email"
                    type="email"
                    placeholder="user@example.com"
                    {...register("email")}
                  />
                  {errors.email && (
                    <p className="text-xs text-[var(--danger)]">{errors.email.message}</p>
                  )}
                </div>
                <div className="w-32 space-y-1.5">
                  <Label htmlFor="role">Role</Label>
                  <select
                    id="role"
                    className="flex h-9 w-full rounded border border-[var(--border)] bg-[var(--bg-elevated)] px-3 py-1 text-sm text-[var(--text)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
                    {...register("role")}
                  >
                    <option value="user">User</option>
                    <option value="admin">Admin</option>
                  </select>
                </div>
              </div>
            </CardContent>
            <CardFooter>
              <Button type="submit" disabled={isSubmitting || createInvite.isPending}>
                {createInvite.isPending ? "Creating…" : "Create invite"}
              </Button>
            </CardFooter>
          </form>
        </Card>

        {/* Invite list */}
        <div>
          <h2 className="text-sm font-medium text-[var(--text-muted)] mb-3">Recent invites</h2>
          {isLoading ? (
            <p className="text-sm text-[var(--text-muted)]">Loading…</p>
          ) : (
            <div className="rounded-lg border border-[var(--border)] overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] bg-[var(--bg-subtle)]">
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Email</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Role</th>
                    <th className="text-left px-4 py-2.5 font-medium text-[var(--text-muted)]">Status</th>
                    <th className="text-right px-4 py-2.5 font-medium text-[var(--text-muted)]">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {data?.items.map((inv) => {
                    const isUsed = !!inv.used_at;
                    const isExpired = !isUsed && new Date(inv.expires_at) < new Date();
                    const statusLabel = isUsed ? "Accepted" : isExpired ? "Expired" : "Pending";
                    const statusColor = isUsed
                      ? "text-[var(--success)]"
                      : isExpired
                      ? "text-[var(--text-subtle)]"
                      : "text-[var(--warning)]";

                    return (
                      <tr
                        key={inv.token_hash}
                        className="border-b border-[var(--border)] last:border-0 hover:bg-[var(--bg-subtle)] transition-colors"
                      >
                        <td className="px-4 py-3 text-[var(--text)]">{inv.email}</td>
                        <td className="px-4 py-3 text-[var(--text-muted)] capitalize">{inv.role}</td>
                        <td className={`px-4 py-3 ${statusColor}`}>{statusLabel}</td>
                        <td className="px-4 py-3 text-right">
                          {!isUsed && !isExpired && (
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => revokeInvite.mutate(inv.token_hash)}
                            >
                              Revoke
                            </Button>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                  {data?.items.length === 0 && (
                    <tr>
                      <td colSpan={4} className="px-4 py-8 text-center text-[var(--text-muted)]">
                        No invites yet.
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
