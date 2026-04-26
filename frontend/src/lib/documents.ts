import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./api";
import toast from "react-hot-toast";

// ─── Types ────────────────────────────────────────────────────────────────────

export type DocumentStatus =
  | "pending"
  | "parsing"
  | "transcribing"
  | "chunking"
  | "embedding"
  | "analysing"
  | "done"
  | "failed";

export type DocumentKind = "text" | "audio";

export interface Document {
  id: string;
  persona_id: string;
  era_id: string | null;
  kind: DocumentKind;
  mime_type: string;
  title: string | null;
  source: string | null;
  word_count: number | null;
  duration_sec: number | null;
  progress_pct: number | null;
  status: DocumentStatus;
  error: string | null;
  transcript_path: string | null;
  created_at: string;
  ingested_at: string | null;
}

export interface DocumentsListResponse {
  items: Document[];
  next_cursor: string | null;
}

export interface DocumentFilters {
  era_id?: string;
  kind?: DocumentKind;
  status?: DocumentStatus[];
  cursor?: string;
  limit?: number;
}

// ─── Hooks ────────────────────────────────────────────────────────────────────

export function useDocumentsList(personaId: string, filters?: DocumentFilters) {
  const params = new URLSearchParams();
  if (filters?.era_id) params.set("era_id", filters.era_id);
  if (filters?.kind) params.set("kind", filters.kind);
  if (filters?.status?.length) {
    filters.status.forEach((s) => params.append("status", s));
  }
  if (filters?.cursor) params.set("cursor", filters.cursor);
  if (filters?.limit) params.set("limit", String(filters.limit));

  const qs = params.toString();

  return useQuery<DocumentsListResponse, ApiError>({
    queryKey: ["personas", personaId, "documents", filters],
    queryFn: () =>
      api<DocumentsListResponse>(
        `/api/personas/${personaId}/documents${qs ? `?${qs}` : ""}`,
      ),
    enabled: Boolean(personaId),
  });
}

export function useDocument(personaId: string, docId: string) {
  return useQuery<Document, ApiError>({
    queryKey: ["personas", personaId, "documents", docId],
    queryFn: () =>
      api<Document>(`/api/personas/${personaId}/documents/${docId}`),
    enabled: Boolean(personaId) && Boolean(docId),
  });
}

export function useUploadDocument(personaId: string) {
  const qc = useQueryClient();
  return useMutation<
    Document,
    ApiError,
    {
      file: File;
      eraId?: string;
      title?: string;
      source?: string;
      onProgress?: (pct: number) => void;
    }
  >({
    mutationFn: ({ file, eraId, title, source, onProgress }) => {
      return new Promise<Document>((resolve, reject) => {
        const form = new FormData();
        form.append("file", file);
        if (eraId) form.append("era_id", eraId);
        if (title) form.append("title", title);
        if (source) form.append("source", source);

        const xhr = new XMLHttpRequest();
        xhr.open("POST", `/api/personas/${personaId}/documents`);
        xhr.withCredentials = true;

        // CSRF token
        const csrfToken = readCookie("pai_csrf");
        if (csrfToken) xhr.setRequestHeader("X-CSRF-Token", csrfToken);

        // Required idempotency key
        xhr.setRequestHeader("Idempotency-Key", crypto.randomUUID());

        if (onProgress) {
          xhr.upload.addEventListener("progress", (e) => {
            if (e.lengthComputable) {
              onProgress(Math.round((e.loaded / e.total) * 100));
            }
          });
        }

        xhr.addEventListener("load", () => {
          if (xhr.status >= 200 && xhr.status < 300) {
            try {
              resolve(JSON.parse(xhr.responseText) as Document);
            } catch {
              reject(new ApiError(xhr.status, "parse_error", "Invalid JSON response"));
            }
          } else {
            try {
              const body = JSON.parse(xhr.responseText);
              reject(
                new ApiError(
                  xhr.status,
                  body?.error?.code ?? "unknown",
                  body?.error?.message ?? `HTTP ${xhr.status}`,
                ),
              );
            } catch {
              reject(new ApiError(xhr.status, "unknown", `HTTP ${xhr.status}`));
            }
          }
        });

        xhr.addEventListener("error", () => {
          reject(new ApiError(0, "network_error", "Network error"));
        });

        xhr.send(form);
      });
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "documents"] });
    },
    onError: (e) => {
      if (e.status === 409 && e.code === "duplicate") {
        toast("This document is already uploaded.", { icon: "ℹ️" });
      } else if (e.status === 413) {
        toast.error("File too large or quota exceeded.");
      } else if (e.status === 415) {
        toast.error("Unsupported file type.");
      } else {
        toast.error(e.message || "Upload failed.");
      }
    },
  });
}

export function useDeleteDocument(personaId: string) {
  const qc = useQueryClient();
  return useMutation<void, ApiError, string>({
    mutationFn: (docId) =>
      api(`/api/personas/${personaId}/documents/${docId}`, {
        method: "DELETE",
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "documents"] });
      toast.success("Document deleted.");
    },
    onError: (e) => toast.error(e.message || "Failed to delete document."),
  });
}

export function useReingestDocument(personaId: string) {
  const qc = useQueryClient();
  return useMutation<Document, ApiError, string>({
    mutationFn: (docId) =>
      api<Document>(
        `/api/personas/${personaId}/documents/${docId}/reingest`,
        {
          method: "POST",
          headers: { "Idempotency-Key": crypto.randomUUID() },
        },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["personas", personaId, "documents"] });
      toast.success("Re-ingestion started.");
    },
    onError: (e) => toast.error(e.message || "Failed to re-ingest document."),
  });
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function readCookie(name: string): string | null {
  const match = document.cookie.match(
    new RegExp("(?:^|; )" + name + "=([^;]*)"),
  );
  return match ? decodeURIComponent(match[1]) : null;
}

export function formatDuration(sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export function statusLabel(status: DocumentStatus): string {
  const map: Record<DocumentStatus, string> = {
    pending: "Pending",
    parsing: "Parsing",
    transcribing: "Transcribing",
    chunking: "Chunking",
    embedding: "Embedding",
    analysing: "Analysing",
    done: "Done",
    failed: "Failed",
  };
  return map[status] ?? status;
}

export function statusColor(status: DocumentStatus): string {
  switch (status) {
    case "done":
      return "text-green-600";
    case "failed":
      return "text-red-600";
    case "pending":
      return "text-zinc-500";
    default:
      // parsing, transcribing, chunking, embedding, analysing
      return "text-amber-600";
  }
}
