import { useNavigate, useSearchParams } from "react-router-dom";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import toast from "react-hot-toast";

interface InviteInfo {
  email: string;
  role: string;
  expires_at: string;
}

const schema = z
  .object({
    password: z.string().min(12, "Password must be at least 12 characters."),
    confirm_password: z.string(),
    display_name: z.string().min(1, "Display name is required.").max(80),
  })
  .refine((d) => d.password === d.confirm_password, {
    message: "Passwords do not match.",
    path: ["confirm_password"],
  });

type FormData = z.infer<typeof schema>;

export default function AcceptInvite() {
  const [params] = useSearchParams();
  const token = params.get("token") ?? "";
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: invite, isError, error } = useQuery<InviteInfo, ApiError>({
    queryKey: ["invite", token],
    queryFn: () => api<InviteInfo>(`/api/invites/validate?token=${encodeURIComponent(token)}`),
    enabled: !!token,
    retry: false,
  });

  const accept = useMutation({
    mutationFn: (body: { token: string; password: string; display_name: string }) =>
      api("/api/invites/accept", { method: "POST", body: JSON.stringify(body) }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["me"] });
      navigate("/personas", { replace: true });
    },
    onError: (e: ApiError) => {
      if (e.status === 410) {
        toast.error("This invite has expired or already been used.");
      } else if (e.status === 400) {
        toast.error(e.message);
      } else {
        toast.error("Something went wrong. Please try again.");
      }
    },
  });

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<FormData>({ resolver: zodResolver(schema) });

  async function onSubmit(data: FormData) {
    accept.mutate({ token, password: data.password, display_name: data.display_name });
  }

  if (!token) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-[var(--bg-subtle)] px-4">
        <Card className="w-full max-w-sm">
          <CardHeader>
            <CardTitle>Invalid invite link</CardTitle>
            <CardDescription>No token found. Please use the link from your invitation email.</CardDescription>
          </CardHeader>
        </Card>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-[var(--bg-subtle)] px-4">
        <Card className="w-full max-w-sm">
          <CardHeader>
            <CardTitle>Invite not found</CardTitle>
            <CardDescription>
              {error?.status === 404
                ? "This invite has expired, been revoked, or already used."
                : "Something went wrong loading this invite."}
            </CardDescription>
          </CardHeader>
        </Card>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[var(--bg-subtle)] px-4">
      <div className="w-full max-w-sm">
        <div className="text-center mb-8">
          <h1 className="text-2xl font-semibold text-[var(--text)]">Persona AI</h1>
          <p className="text-sm text-[var(--text-muted)] mt-1">You've been invited</p>
        </div>

        <Card>
          <form onSubmit={handleSubmit(onSubmit)}>
            <CardHeader>
              <CardTitle>Create your account</CardTitle>
              {invite && (
                <CardDescription>
                  Setting up account for <strong>{invite.email}</strong>
                </CardDescription>
              )}
            </CardHeader>

            <CardContent className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="display_name">Display name</Label>
                <Input
                  id="display_name"
                  type="text"
                  autoComplete="name"
                  placeholder="Your name"
                  {...register("display_name")}
                />
                {errors.display_name && (
                  <p className="text-xs text-[var(--danger)]">{errors.display_name.message}</p>
                )}
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="password">Password</Label>
                <Input
                  id="password"
                  type="password"
                  autoComplete="new-password"
                  {...register("password")}
                />
                {errors.password && (
                  <p className="text-xs text-[var(--danger)]">{errors.password.message}</p>
                )}
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="confirm_password">Confirm password</Label>
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
              <Button
                type="submit"
                className="w-full"
                disabled={isSubmitting || accept.isPending || !invite}
              >
                {accept.isPending ? "Creating account…" : "Create account"}
              </Button>
            </CardFooter>
          </form>
        </Card>
      </div>
    </div>
  );
}
