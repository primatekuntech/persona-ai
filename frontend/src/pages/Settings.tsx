import { useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { api, ApiError } from "@/lib/api";
import { useMe, useRevokeAllSessions } from "@/lib/auth";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import toast from "react-hot-toast";

const deleteSchema = z.object({
  password: z.string().min(1, "Password is required."),
  confirm: z.string(),
}).refine((d) => d.confirm === "DELETE", {
  message: "Type DELETE to confirm.",
  path: ["confirm"],
});

type DeleteFormData = z.infer<typeof deleteSchema>;

const pwSchema = z
  .object({
    current_password: z.string().min(1, "Current password is required."),
    new_password: z.string().min(12, "New password must be at least 12 characters."),
    confirm_password: z.string(),
  })
  .refine((d) => d.new_password === d.confirm_password, {
    message: "Passwords do not match.",
    path: ["confirm_password"],
  });

type PwFormData = z.infer<typeof pwSchema>;

export default function Settings() {
  const { data: me } = useMe();
  const revokeAll = useRevokeAllSessions();
  const [revokeConfirm, setRevokeConfirm] = useState(false);
  const [exportStatus, setExportStatus] = useState<"idle" | "pending" | "done">("idle");
  const [exportUrl, setExportUrl] = useState<string | null>(null);
  const exportPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<PwFormData>({ resolver: zodResolver(pwSchema) });

  const {
    register: registerDelete,
    handleSubmit: handleDelete,
    formState: { errors: deleteErrors, isSubmitting: deleteSubmitting },
  } = useForm<DeleteFormData>({ resolver: zodResolver(deleteSchema) });

  async function onExportData() {
    setExportStatus("pending");
    setExportUrl(null);
    try {
      const res = await api<{ export_url?: string; status?: string }>(
        "/api/auth/export",
        { method: "POST" },
      );
      if (res.export_url) {
        setExportStatus("done");
        setExportUrl(res.export_url);
        return;
      }
      // Poll if the backend returns a job status instead of an immediate URL
      let attempts = 0;
      exportPollRef.current = setInterval(async () => {
        attempts++;
        try {
          const poll = await api<{ export_url?: string; status?: string }>("/api/auth/export");
          if (poll.export_url) {
            setExportStatus("done");
            setExportUrl(poll.export_url);
            if (exportPollRef.current) clearInterval(exportPollRef.current);
          }
        } catch {
          // continue polling
        }
        if (attempts >= 24) {
          // 2 minutes
          if (exportPollRef.current) clearInterval(exportPollRef.current);
          setExportStatus("idle");
          toast.error("Export timed out. Please try again.");
        }
      }, 5000);
    } catch (e) {
      setExportStatus("idle");
      if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Export failed. Please try again.");
      }
    }
  }

  async function onDeleteAccount(data: DeleteFormData) {
    try {
      await api("/api/auth/account", {
        method: "DELETE",
        body: JSON.stringify({ password: data.password }),
      });
      toast.success("Account deleted.");
      window.location.href = "/login";
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        toast.error("Password is incorrect.");
      } else if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Something went wrong. Please try again.");
      }
    }
  }

  async function onChangePassword(data: PwFormData) {
    try {
      // Verify current password by attempting a login (non-destructive check).
      // If it succeeds the server returns 204 + a new session cookie; if wrong → 401.
      await api("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ email: me?.email, password: data.current_password }),
      });
      // Login succeeded — now request a reset token for the account so we can set
      // the new password via the reset endpoint.  The reset email arrives in the
      // background; for now tell the user to check their inbox.
      await api("/api/auth/password/forgot", {
        method: "POST",
        body: JSON.stringify({ email: me?.email }),
      });
      toast.success("A password-reset link has been sent to your email address.");
      reset();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        toast.error("Current password is incorrect.");
      } else if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Something went wrong. Please try again.");
      }
    }
  }

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Account settings</h1>
      </div>

      <div className="flex-1 overflow-y-auto p-6 max-w-2xl space-y-6">
        {/* Profile info */}
        <Card>
          <CardHeader>
            <CardTitle>Profile</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <div className="flex items-center justify-between py-1">
              <span className="text-sm text-[var(--text-muted)]">Email</span>
              <span className="text-sm text-[var(--text)]">{me?.email}</span>
            </div>
            <div className="flex items-center justify-between py-1">
              <span className="text-sm text-[var(--text-muted)]">Display name</span>
              <span className="text-sm text-[var(--text)]">{me?.display_name ?? "—"}</span>
            </div>
            <div className="flex items-center justify-between py-1">
              <span className="text-sm text-[var(--text-muted)]">Role</span>
              <span className="text-sm text-[var(--text)] capitalize">{me?.role}</span>
            </div>
          </CardContent>
        </Card>

        {/* Change password */}
        <Card>
          <form onSubmit={handleSubmit(onChangePassword)}>
            <CardHeader>
              <CardTitle>Change password</CardTitle>
              <CardDescription>
                Changing your password will sign you out of all other sessions.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="current_password">Current password</Label>
                <Input
                  id="current_password"
                  type="password"
                  autoComplete="current-password"
                  {...register("current_password")}
                />
                {errors.current_password && (
                  <p className="text-xs text-[var(--danger)]">{errors.current_password.message}</p>
                )}
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="new_password">New password</Label>
                <Input
                  id="new_password"
                  type="password"
                  autoComplete="new-password"
                  {...register("new_password")}
                />
                {errors.new_password && (
                  <p className="text-xs text-[var(--danger)]">{errors.new_password.message}</p>
                )}
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="confirm_password">Confirm new password</Label>
                <Input
                  id="confirm_password"
                  type="password"
                  autoComplete="new-password"
                  {...register("confirm_password")}
                />
                {errors.confirm_password && (
                  <p className="text-xs text-[var(--danger)]">{errors.confirm_password.message}</p>
                )}
              </div>
            </CardContent>
            <CardFooter>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Updating…" : "Update password"}
              </Button>
            </CardFooter>
          </form>
        </Card>

        {/* Sessions */}
        <Card>
          <CardHeader>
            <CardTitle>Sessions</CardTitle>
            <CardDescription>
              Sign out of all active sessions across all devices.
            </CardDescription>
          </CardHeader>
          <CardFooter>
            {revokeConfirm ? (
              <div className="flex items-center gap-3">
                <span className="text-sm text-[var(--text-muted)]">Sign out everywhere?</span>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => revokeAll.mutate()}
                  disabled={revokeAll.isPending}
                >
                  {revokeAll.isPending ? "Revoking…" : "Yes, sign out all"}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setRevokeConfirm(false)}
                >
                  Cancel
                </Button>
              </div>
            ) : (
              <Button
                variant="outline"
                onClick={() => setRevokeConfirm(true)}
              >
                Sign out everywhere
              </Button>
            )}
          </CardFooter>
        </Card>

        {/* Export account data */}
        <Card>
          <CardHeader>
            <CardTitle>Export account data</CardTitle>
            <CardDescription>
              Download a ZIP archive of all your personas, documents, transcripts, and chat
              history. Exports are generated on demand and may take a moment.
            </CardDescription>
          </CardHeader>
          <CardFooter className="flex items-center gap-4">
            <Button
              variant="outline"
              onClick={onExportData}
              disabled={exportStatus === "pending"}
            >
              {exportStatus === "pending" ? "Preparing export…" : "Export my data"}
            </Button>
            {exportStatus === "done" && exportUrl && (
              <a
                href={exportUrl}
                download
                className="text-sm font-medium text-[var(--accent)] underline underline-offset-2"
              >
                Download export
              </a>
            )}
          </CardFooter>
        </Card>

        {/* Delete account */}
        <Card>
          <form onSubmit={handleDelete(onDeleteAccount)}>
            <CardHeader>
              <CardTitle className="text-[var(--danger)]">Delete account</CardTitle>
              <CardDescription>
                Permanently delete your account and all associated data. This action cannot be
                undone.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="delete_password">Current password</Label>
                <Input
                  id="delete_password"
                  type="password"
                  autoComplete="current-password"
                  {...registerDelete("password")}
                />
                {deleteErrors.password && (
                  <p className="text-xs text-[var(--danger)]">{deleteErrors.password.message}</p>
                )}
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="delete_confirm">
                  Type <strong>DELETE</strong> to confirm
                </Label>
                <Input
                  id="delete_confirm"
                  type="text"
                  placeholder="DELETE"
                  {...registerDelete("confirm")}
                />
                {deleteErrors.confirm && (
                  <p className="text-xs text-[var(--danger)]">{deleteErrors.confirm.message}</p>
                )}
              </div>
            </CardContent>
            <CardFooter>
              <Button type="submit" variant="destructive" disabled={deleteSubmitting}>
                {deleteSubmitting ? "Deleting…" : "Delete my account"}
              </Button>
            </CardFooter>
          </form>
        </Card>
      </div>
    </div>
  );
}
