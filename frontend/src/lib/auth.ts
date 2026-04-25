import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./api";
import toast from "react-hot-toast";

export interface Me {
  user_id: string;
  email: string;
  role: "admin" | "user";
  display_name: string | null;
}

export function useMe() {
  return useQuery<Me, ApiError>({
    queryKey: ["me"],
    queryFn: () => api<Me>("/api/auth/me"),
    retry: (failureCount, error) => {
      // Don't retry on 401 — user is just not logged in
      if (error instanceof ApiError && error.status === 401) return false;
      return failureCount < 1;
    },
  });
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: { email: string; password: string }) =>
      api("/api/auth/login", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["me"] }),
    onError: (e: ApiError) => {
      if (e.status === 429) {
        toast.error(e.message);
      } else {
        toast.error("Invalid email or password.");
      }
    },
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api("/api/auth/logout", { method: "POST" }),
    onSuccess: () => {
      qc.clear();
      window.location.href = "/login";
    },
  });
}

export function useRevokeAllSessions() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () =>
      api("/api/auth/sessions/revoke-all", { method: "POST" }),
    onSuccess: () => {
      toast.success("All sessions signed out.");
      qc.clear();
      window.location.href = "/login";
    },
    onError: () => toast.error("Failed to revoke sessions."),
  });
}
