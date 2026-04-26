import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "./api";
import toast from "react-hot-toast";

export interface ChatSession {
  id: string;
  persona_id: string;
  era_id: string | null;
  user_id: string;
  title: string | null;
  model_id: string;
  temperature: number;
  top_p: number;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  session_id: string;
  user_id: string;
  role: "user" | "assistant";
  content: string;
  retrieved_chunk_ids: string[];
  tokens_in: number | null;
  tokens_out: number | null;
  created_at: string;
}

export interface ChatListResponse {
  items: ChatSession[];
  next_cursor: string | null;
}

export interface SessionResponse {
  session: ChatSession;
  messages: Message[];
  next_cursor: string | null;
}

// ─── Hooks ────────────────────────────────────────────────────────────────────

export function useChats(personaId: string) {
  return useQuery<ChatListResponse, ApiError>({
    queryKey: ["chats", personaId],
    queryFn: () => api<ChatListResponse>(`/api/personas/${personaId}/chats`),
    enabled: Boolean(personaId),
  });
}

export function useChatSession(sessionId: string | undefined) {
  return useQuery<SessionResponse, ApiError>({
    queryKey: ["chat", sessionId],
    queryFn: () => api<SessionResponse>(`/api/chats/${sessionId}`),
    enabled: Boolean(sessionId),
  });
}

export function useCreateChat(personaId: string) {
  const qc = useQueryClient();
  return useMutation<
    ChatSession,
    ApiError,
    { era_id?: string | null; temperature?: number; top_p?: number }
  >({
    mutationFn: (body) =>
      api<ChatSession>(`/api/personas/${personaId}/chats`, {
        method: "POST",
        body: JSON.stringify(body),
        headers: { "Idempotency-Key": crypto.randomUUID() },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["chats", personaId] });
    },
    onError: (e) => {
      toast.error(e.message || "Failed to start chat.");
    },
  });
}

export function useDeleteChat() {
  const qc = useQueryClient();
  return useMutation<void, ApiError, { sessionId: string; personaId: string }>(
    {
      mutationFn: ({ sessionId }) =>
        api<void>(`/api/chats/${sessionId}`, { method: "DELETE" }),
      onSuccess: (_, { personaId }) => {
        qc.invalidateQueries({ queryKey: ["chats", personaId] });
      },
    },
  );
}

// ─── SSE streaming ────────────────────────────────────────────────────────────

export type SseEvent =
  | {
      type: "meta";
      data: {
        assistant_message_id: string;
        retrieved_chunk_ids: string[];
        synthetic?: boolean;
        replay?: boolean;
      };
    }
  | { type: "token"; data: { t: string } }
  | {
      type: "done";
      data: {
        assistant_message_id: string;
        tokens_in: number;
        tokens_out: number;
        finish_reason: string;
      };
    }
  | { type: "error"; data: { code: string; message: string } };

function parseFrame(frame: string): SseEvent | null {
  let type = "message";
  let rawData: string | null = null;
  for (const line of frame.split("\n")) {
    if (line.startsWith("event: ")) type = line.slice(7).trim();
    else if (line.startsWith("data: ")) rawData = line.slice(6);
    // lines starting with ":" are heartbeat comments — ignore
  }
  if (!rawData) return null;
  try {
    const data = JSON.parse(rawData);
    return { type, data } as SseEvent;
  } catch {
    return null;
  }
}

function getCsrfToken(): string {
  const match = document.cookie.match(/(?:^|;\s*)pai_csrf=([^;]+)/);
  return match ? decodeURIComponent(match[1]) : "";
}

export async function streamChat(
  sessionId: string,
  content: string,
  onEvent: (ev: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`/api/chats/${sessionId}/messages`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": getCsrfToken(),
      "Idempotency-Key": crypto.randomUUID(),
      Accept: "text/event-stream",
    },
    body: JSON.stringify({ content }),
    signal,
  });

  if (!res.ok || !res.body) {
    let errMsg = "Request failed";
    try {
      const err = await res.json();
      errMsg = err?.error?.message ?? errMsg;
    } catch {
      /* ignore */
    }
    throw new Error(errMsg);
  }

  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += value;
    let idx: number;
    while ((idx = buffer.indexOf("\n\n")) !== -1) {
      const frame = buffer.slice(0, idx);
      buffer = buffer.slice(idx + 2);
      const ev = parseFrame(frame);
      if (!ev) continue;
      if (ev.type === "done") {
        onEvent(ev);
        return;
      }
      if (ev.type === "error") {
        onEvent(ev);
        throw new Error(
          (ev as { type: "error"; data: { code: string; message: string } })
            .data.message,
        );
      }
      onEvent(ev);
    }
  }
}
