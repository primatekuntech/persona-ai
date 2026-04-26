import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./api";
import toast from "react-hot-toast";

// ─── Types ────────────────────────────────────────────────────────────────────

export interface Persona {
  id: string;
  user_id: string;
  name: string;
  relation: string | null;
  description: string | null;
  avatar_path: string | null;
  birth_year: number | null;
  created_at: string;
  updated_at: string;
}

export interface PersonaWithCounts extends Persona {
  doc_count: number;
  era_count: number;
}

export interface PersonasListResponse {
  items: PersonaWithCounts[];
  next_cursor: string | null;
  total_estimate?: number;
}

export interface Era {
  id: string;
  persona_id: string;
  user_id: string;
  label: string;
  start_date: string | null;
  end_date: string | null;
  description: string | null;
  created_at: string;
  updated_at: string;
}

// ─── Persona hooks ────────────────────────────────────────────────────────────

export function usePersonasList() {
  return useQuery<PersonasListResponse, ApiError>({
    queryKey: ["personas"],
    queryFn: () => api<PersonasListResponse>("/api/personas?limit=200"),
  });
}

export function usePersona(id: string) {
  return useQuery<Persona, ApiError>({
    queryKey: ["personas", id],
    queryFn: () => api<Persona>(`/api/personas/${id}`),
    enabled: Boolean(id),
  });
}

export function useCreatePersona() {
  const qc = useQueryClient();
  return useMutation<Persona, ApiError, {
    name: string;
    relation?: string | null;
    description?: string | null;
    birth_year?: number | null;
  }>({
    mutationFn: (body) =>
      api<Persona>("/api/personas", {
        method: "POST",
        body: JSON.stringify(body),
        headers: { "Idempotency-Key": crypto.randomUUID() },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Persona created.");
    },
    onError: (e) => {
      if (e.status === 409) toast.error("A persona with that name already exists.");
      else toast.error(e.message || "Failed to create persona.");
    },
  });
}

export function usePatchPersona(id: string) {
  const qc = useQueryClient();
  return useMutation<Persona, ApiError, Partial<{
    name: string;
    relation: string | null;
    description: string | null;
    birth_year: number | null;
  }>>({
    mutationFn: (body) =>
      api<Persona>(`/api/personas/${id}`, {
        method: "PATCH",
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", id] });
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Persona updated.");
    },
    onError: (e) => toast.error(e.message || "Failed to update persona."),
  });
}

export function useUploadAvatar(id: string) {
  const qc = useQueryClient();
  return useMutation<{ avatar_path: string }, ApiError, File>({
    mutationFn: async (file) => {
      const form = new FormData();
      form.append("avatar", file);
      const res = await fetch(`/api/personas/${id}/avatar`, {
        method: "POST",
        credentials: "include",
        body: form,
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new ApiError(
          res.status,
          body?.error?.code ?? "unknown",
          body?.error?.message ?? `HTTP ${res.status}`,
        );
      }
      return res.json();
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", id] });
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Avatar updated.");
    },
    onError: (e) => toast.error(e.message || "Failed to upload avatar."),
  });
}

export function useDeletePersona() {
  const qc = useQueryClient();
  return useMutation<void, ApiError, string>({
    mutationFn: (id) => api(`/api/personas/${id}`, { method: "DELETE" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Persona deleted.");
    },
    onError: (e) => toast.error(e.message || "Failed to delete persona."),
  });
}

// ─── Era hooks ────────────────────────────────────────────────────────────────

export function useErasList(personaId: string) {
  return useQuery<Era[], ApiError>({
    queryKey: ["personas", personaId, "eras"],
    queryFn: () => api<Era[]>(`/api/personas/${personaId}/eras`),
    enabled: Boolean(personaId),
  });
}

export function useCreateEra(personaId: string) {
  const qc = useQueryClient();
  return useMutation<Era, ApiError, {
    label: string;
    start_date?: string | null;
    end_date?: string | null;
    description?: string | null;
  }>({
    mutationFn: (body) =>
      api<Era>(`/api/personas/${personaId}/eras`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "eras"] });
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Era created.");
    },
    onError: (e) => {
      if (e.status === 409) toast.error("An era with that label already exists.");
      else toast.error(e.message || "Failed to create era.");
    },
  });
}

export function usePatchEra(personaId: string) {
  const qc = useQueryClient();
  return useMutation<Era, ApiError, {
    eraId: string;
    body: Partial<{
      label: string;
      start_date: string | null;
      end_date: string | null;
      description: string | null;
    }>;
  }>({
    mutationFn: ({ eraId, body }) =>
      api<Era>(`/api/personas/${personaId}/eras/${eraId}`, {
        method: "PATCH",
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "eras"] });
      toast.success("Era updated.");
    },
    onError: (e) => toast.error(e.message || "Failed to update era."),
  });
}

export function useDeleteEra(personaId: string) {
  const qc = useQueryClient();
  return useMutation<void, ApiError, string>({
    mutationFn: (eraId) =>
      api(`/api/personas/${personaId}/eras/${eraId}`, { method: "DELETE" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "eras"] });
      qc.invalidateQueries({ queryKey: ["personas"] });
      toast.success("Era deleted.");
    },
    onError: (e) => toast.error(e.message || "Failed to delete era."),
  });
}
