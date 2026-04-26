import { useState, useRef, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { usePersona, useErasList } from "@/lib/personas";
import {
  useChats,
  useChatSession,
  useCreateChat,
  useDeleteChat,
  streamChat,
  type Message,
  type SseEvent,
  type ChatSession,
} from "@/lib/chats";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  MessageSquare,
  Plus,
  Trash2,
  Loader2,
  ChevronDown,
  ChevronUp,
  Send,
  Download,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useQueryClient } from "@tanstack/react-query";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function triggerExport(sessionId: string, format: "md" | "docx", messageId?: string) {
  const params = new URLSearchParams({ format });
  if (messageId) params.set("message_ids", messageId);
  window.location.href = `/api/chats/${sessionId}/export?${params}`;
}

// ─── Session export dialog ────────────────────────────────────────────────────

function ExportDialog({
  session,
  messages,
  onClose,
}: {
  session: ChatSession;
  messages: Message[];
  onClose: () => void;
}) {
  const asstMessages = messages.filter((m) => m.role === "assistant" && m.content);
  const [selected, setSelected] = useState<Set<string>>(
    new Set(asstMessages.map((m) => m.id))
  );
  const [format, setFormat] = useState<"md" | "docx">("docx");
  const [title, setTitle] = useState(session.title ?? "");

  const toggle = (id: string) =>
    setSelected((s) => {
      const next = new Set(s);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  const handleExport = () => {
    const ids = Array.from(selected).join(",");
    const params = new URLSearchParams({ format });
    if (ids) params.set("message_ids", ids);
    if (title) params.set("title", title);
    window.location.href = `/api/chats/${session.id}/export?${params}`;
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="bg-[var(--bg)] border border-[var(--border)] rounded-lg shadow-xl w-full max-w-md p-5">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-sm font-semibold text-[var(--text)]">Export session</h2>
          <button onClick={onClose} className="text-[var(--text-muted)] hover:text-[var(--text)]">
            <X size={14} />
          </button>
        </div>

        <label className="block text-xs text-[var(--text-subtle)] mb-1">Title</label>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="(auto)"
          className="w-full text-sm bg-[var(--bg-subtle)] border border-[var(--border)] rounded px-3 py-1.5 mb-4 text-[var(--text)] focus:outline-none focus:border-[var(--accent)]"
        />

        <label className="block text-xs text-[var(--text-subtle)] mb-1">Messages</label>
        <div className="border border-[var(--border)] rounded divide-y divide-[var(--border)] max-h-48 overflow-y-auto mb-4">
          {asstMessages.length === 0 && (
            <p className="text-xs text-[var(--text-muted)] px-3 py-2">No assistant messages yet.</p>
          )}
          {asstMessages.map((msg) => (
            <label key={msg.id} className="flex items-start gap-2 px-3 py-2 cursor-pointer hover:bg-[var(--bg-subtle)]">
              <input
                type="checkbox"
                checked={selected.has(msg.id)}
                onChange={() => toggle(msg.id)}
                className="mt-0.5 shrink-0"
              />
              <span className="text-xs text-[var(--text)] truncate">
                {msg.content.slice(0, 80)}{msg.content.length > 80 ? "…" : ""}
              </span>
            </label>
          ))}
        </div>

        <div className="flex items-center gap-2 mb-5">
          {(["md", "docx"] as const).map((f) => (
            <button
              key={f}
              onClick={() => setFormat(f)}
              className={cn(
                "px-3 py-1 rounded text-xs font-medium border transition-colors",
                format === f
                  ? "bg-[var(--accent)] text-white border-[var(--accent)]"
                  : "border-[var(--border)] text-[var(--text-muted)] hover:border-[var(--accent)]"
              )}
            >
              .{f}
            </button>
          ))}
        </div>

        <div className="flex gap-2 justify-end">
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" disabled={selected.size === 0} onClick={handleExport}>
            <Download size={12} className="mr-1.5" />
            Export
          </Button>
        </div>
      </div>
    </div>
  );
}

// ─── Message bubble ───────────────────────────────────────────────────────────

function MessageBubble({
  message,
  streamingContent,
  sessionId,
}: {
  message?: Message;
  streamingContent?: string;
  sessionId?: string;
}) {
  const [citationsOpen, setCitationsOpen] = useState(false);
  const isUser = message?.role === "user";
  const content = message?.content ?? streamingContent ?? "";
  const chunkIds = message?.retrieved_chunk_ids ?? [];

  return (
    <div className={cn("group flex flex-col", isUser ? "items-end" : "items-start")}>
      <div
        className={cn(
          "max-w-[75%] rounded-lg px-4 py-2.5 text-sm leading-relaxed",
          isUser
            ? "bg-[var(--accent)] text-white"
            : "bg-[var(--bg-elevated)] text-[var(--text)] border border-[var(--border)]",
        )}
      >
        <p className="whitespace-pre-wrap">
          {content}
          {!message && streamingContent !== undefined && (
            <span className="inline-block w-0.5 h-4 bg-current align-text-bottom ml-0.5 animate-pulse" />
          )}
        </p>
        {!isUser && chunkIds.length > 0 && (
          <div className="mt-2 pt-2 border-t border-[var(--border)]">
            <button
              onClick={() => setCitationsOpen((o) => !o)}
              className="flex items-center gap-1 text-xs text-[var(--text-muted)] hover:text-[var(--text)] transition-colors"
            >
              {citationsOpen ? <ChevronUp size={11} /> : <ChevronDown size={11} />}
              {chunkIds.length} source{chunkIds.length !== 1 ? "s" : ""}
            </button>
            {citationsOpen && (
              <ul className="mt-1 space-y-0.5">
                {chunkIds.map((id) => (
                  <li key={id} className="text-xs text-[var(--text-subtle)] font-mono truncate">
                    {id}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
      {/* Per-message export actions (assistant messages only, after stream) */}
      {!isUser && message && sessionId && (
        <div className="flex items-center gap-1 mt-0.5 invisible group-hover:visible">
          <button
            onClick={() => navigator.clipboard.writeText(content)}
            className="text-xs text-[var(--text-muted)] hover:text-[var(--text)] px-1.5 py-0.5 rounded transition-colors"
          >
            Copy
          </button>
          <button
            onClick={() => triggerExport(sessionId, "md", message.id)}
            className="text-xs text-[var(--text-muted)] hover:text-[var(--text)] px-1.5 py-0.5 rounded transition-colors"
          >
            .md
          </button>
          <button
            onClick={() => triggerExport(sessionId, "docx", message.id)}
            className="text-xs text-[var(--text-muted)] hover:text-[var(--text)] px-1.5 py-0.5 rounded transition-colors"
          >
            .docx
          </button>
        </div>
      )}
    </div>
  );
}

// ─── Chat view ────────────────────────────────────────────────────────────────

function ChatView({ sessionId }: { sessionId: string }) {
  const { id: personaId } = useParams<{ id: string }>();
  const qc = useQueryClient();
  const { data, isLoading } = useChatSession(sessionId);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [streamingText, setStreamingText] = useState<string>("");
  const [optimisticUserMsg, setOptimisticUserMsg] = useState<string | null>(
    null,
  );
  const abortRef = useRef<AbortController | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  // Scroll to bottom when messages change or streaming updates
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [data?.messages, streamingText, optimisticUserMsg]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || streaming) return;

    setInput("");
    setOptimisticUserMsg(text);
    setStreamingText("");
    setStreaming(true);

    const abort = new AbortController();
    abortRef.current = abort;

    try {
      await streamChat(
        sessionId,
        text,
        (ev: SseEvent) => {
          if (ev.type === "token") {
            setStreamingText((prev) => prev + ev.data.t);
          }
        },
        abort.signal,
      );
    } catch (err) {
      // Silently absorb abort errors
      if (err instanceof Error && err.name === "AbortError") return;
    } finally {
      setStreaming(false);
      setStreamingText("");
      setOptimisticUserMsg(null);
      // Refresh session messages
      qc.invalidateQueries({ queryKey: ["chat", sessionId] });
      if (personaId) {
        qc.invalidateQueries({ queryKey: ["chats", personaId] });
      }
    }
  }, [input, streaming, sessionId, qc, personaId]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 size={20} className="animate-spin text-[var(--text-muted)]" />
      </div>
    );
  }

  const messages = data?.messages ?? [];

  return (
    <div className="flex flex-col h-full">
      {/* Message thread */}
      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {messages.length === 0 && !optimisticUserMsg && (
          <div className="flex-1 flex items-center justify-center text-sm text-[var(--text-muted)]">
            Start the conversation below.
          </div>
        )}
        {messages.map((msg) => (
          <MessageBubble key={msg.id} message={msg} sessionId={sessionId} />
        ))}
        {optimisticUserMsg && (
          <MessageBubble
            message={{
              id: "optimistic-user",
              session_id: sessionId,
              user_id: "",
              role: "user",
              content: optimisticUserMsg,
              retrieved_chunk_ids: [],
              tokens_in: null,
              tokens_out: null,
              created_at: new Date().toISOString(),
            }}
          />
        )}
        {streaming && (
          <MessageBubble streamingContent={streamingText} />
        )}
        <div ref={bottomRef} />
      </div>

      {/* Input area */}
      <div className="border-t border-[var(--border)] p-3 bg-[var(--bg)]">
        <div className="flex gap-2 items-end">
          <Textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Message the persona… (Enter to send, Shift+Enter for newline)"
            className="flex-1 min-h-[60px] max-h-[160px] resize-none text-sm"
            disabled={streaming}
          />
          {data && messages.length > 0 && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setExportDialogOpen(true)}
              className="shrink-0"
              title="Export session"
            >
              <Download size={14} />
            </Button>
          )}
          <Button
            onClick={handleSend}
            disabled={!input.trim() || streaming}
            size="sm"
            className="shrink-0"
          >
            {streaming ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Send size={14} />
            )}
          </Button>
        </div>
      </div>
      {exportDialogOpen && data && (
        <ExportDialog
          session={data.session}
          messages={messages}
          onClose={() => setExportDialogOpen(false)}
        />
      )}
    </div>
  );
}

// ─── Main Chat page ───────────────────────────────────────────────────────────

export default function ChatPage() {
  const { id: personaId, sessionId } = useParams<{
    id: string;
    sessionId?: string;
  }>();
  const navigate = useNavigate();
  const { data: persona } = usePersona(personaId!);
  const { data: erasData } = useErasList(personaId!);
  const { data: chatsData, isLoading: chatsLoading } = useChats(personaId!);
  const createChat = useCreateChat(personaId!);
  const deleteChat = useDeleteChat();
  const [selectedEraId, setSelectedEraId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);

  const sessions = chatsData?.items ?? [];
  const eras = erasData ?? [];

  const handleNewChat = async () => {
    try {
      const session = await createChat.mutateAsync({
        era_id: selectedEraId,
      });
      navigate(`/personas/${personaId}/chat/${session.id}`);
    } catch {
      // Error already toasted by hook
    }
  };

  const handleDeleteSession = async (
    e: React.MouseEvent,
    sid: string,
  ) => {
    e.stopPropagation();
    e.preventDefault();
    if (!personaId) return;
    await deleteChat.mutateAsync({ sessionId: sid, personaId });
    if (sessionId === sid) {
      navigate(`/personas/${personaId}/chat`);
    }
  };

  return (
    <div className="flex h-full overflow-hidden">
      {/* Session sidebar */}
      <aside
        className={cn(
          "flex flex-col border-r border-[var(--border)] bg-[var(--bg-elevated)] transition-all",
          sidebarOpen ? "w-64 shrink-0" : "w-0 overflow-hidden",
        )}
      >
        <div className="p-3 border-b border-[var(--border)] space-y-2">
          <div className="flex items-center justify-between">
            <h2 className="text-xs font-semibold text-[var(--text-subtle)] uppercase tracking-wider">
              Chats
            </h2>
            <button
              onClick={() => setSidebarOpen(false)}
              className="text-[var(--text-muted)] hover:text-[var(--text)] p-1 rounded"
            >
              <ChevronDown size={13} className="-rotate-90" />
            </button>
          </div>

          {/* Era picker */}
          {eras.length > 0 && (
            <select
              value={selectedEraId ?? ""}
              onChange={(e) =>
                setSelectedEraId(e.target.value || null)
              }
              className="w-full text-xs bg-[var(--bg)] border border-[var(--border)] rounded px-2 py-1.5 text-[var(--text)]"
            >
              <option value="">All eras</option>
              {eras.map((era) => (
                <option key={era.id} value={era.id}>
                  {era.label}
                </option>
              ))}
            </select>
          )}

          <Button
            size="sm"
            onClick={handleNewChat}
            disabled={createChat.isPending}
            className="w-full justify-start gap-2 text-xs"
          >
            {createChat.isPending ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <Plus size={12} />
            )}
            New chat
          </Button>
        </div>

        {/* Session list */}
        <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
          {chatsLoading && (
            <div className="flex items-center justify-center py-4">
              <Loader2
                size={16}
                className="animate-spin text-[var(--text-muted)]"
              />
            </div>
          )}
          {sessions.length === 0 && !chatsLoading && (
            <p className="text-xs text-[var(--text-muted)] px-2 py-2">
              No chats yet.
            </p>
          )}
          {sessions.map((session) => (
            <div
              key={session.id}
              onClick={() =>
                navigate(`/personas/${personaId}/chat/${session.id}`)
              }
              className={cn(
                "group flex items-center justify-between gap-1 px-2 py-1.5 rounded cursor-pointer text-xs transition-colors",
                sessionId === session.id
                  ? "bg-[var(--bg-subtle)] text-[var(--text)]"
                  : "text-[var(--text-muted)] hover:bg-[var(--bg-subtle)] hover:text-[var(--text)]",
              )}
            >
              <div className="flex items-center gap-1.5 overflow-hidden">
                <MessageSquare size={11} className="shrink-0" />
                <span className="truncate">
                  {session.title ?? "New chat"}
                </span>
              </div>
              <button
                onClick={(e) => handleDeleteSession(e, session.id)}
                className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:text-red-500 transition-all"
              >
                <Trash2 size={11} />
              </button>
            </div>
          ))}
        </div>
      </aside>

      {/* Main area */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Top bar */}
        <div className="h-12 flex items-center gap-3 px-4 border-b border-[var(--border)] bg-[var(--bg)] shrink-0">
          {!sidebarOpen && (
            <button
              onClick={() => setSidebarOpen(true)}
              className="text-[var(--text-muted)] hover:text-[var(--text)] p-1 rounded"
            >
              <ChevronDown size={14} className="rotate-90" />
            </button>
          )}
          <h1 className="text-sm font-medium text-[var(--text)] truncate">
            {sessionId
              ? (sessions.find((s) => s.id === sessionId)?.title ??
                "Chat")
              : `Chat with ${persona?.name ?? "persona"}`}
          </h1>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-hidden">
          {sessionId ? (
            <ChatView sessionId={sessionId} />
          ) : (
            <div className="h-full flex flex-col items-center justify-center gap-4 text-center px-8">
              <MessageSquare
                size={36}
                className="text-[var(--text-muted)]"
              />
              <div>
                <p className="text-sm font-medium text-[var(--text)]">
                  Chat with {persona?.name ?? "this persona"}
                </p>
                <p className="text-xs text-[var(--text-muted)] mt-1">
                  Select a chat from the sidebar or start a new one.
                </p>
              </div>
              <Button size="sm" onClick={handleNewChat}>
                <Plus size={14} className="mr-1.5" />
                New chat
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
