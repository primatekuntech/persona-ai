import { useCallback, useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { useErasList } from "@/lib/personas";
import {
  Document,
  DocumentStatus,
  statusColor,
  statusLabel,
  useUploadDocument,
} from "@/lib/documents";
import { Select } from "@/components/ui/select";

// ─── Types ────────────────────────────────────────────────────────────────────

type UploadState = "queued" | "uploading" | "processing" | "done" | "failed";

interface UploadItem {
  id: string; // local ID (not doc ID yet)
  file: File;
  uploadPct: number;
  ingestPct: number | null;
  state: UploadState;
  status: DocumentStatus | null;
  documentId: string | null;
  error: string | null;
}

const MAX_CONCURRENT = 3;

// ─── Component ────────────────────────────────────────────────────────────────

export default function Upload() {
  const { id: personaId } = useParams<{ id: string }>();
  const { data: eras } = useErasList(personaId!);

  const [eraId, setEraId] = useState("");
  const [source, setSource] = useState("");
  const [items, setItems] = useState<UploadItem[]>([]);
  const [isDragging, setIsDragging] = useState(false);

  const uploadMutation = useUploadDocument(personaId!);
  const activeCount = useRef(0);
  const queueRef = useRef<UploadItem[]>([]);
  const sseRef = useRef<EventSource | null>(null);

  // ── SSE subscription ──────────────────────────────────────────────────────

  useEffect(() => {
    if (!personaId) return;
    const es = new EventSource(`/api/personas/${personaId}/documents/events`, {
      withCredentials: true,
    });
    sseRef.current = es;

    es.onmessage = (ev) => {
      try {
        const data = JSON.parse(ev.data) as {
          document_id: string;
          status: DocumentStatus;
          progress_pct: number | null;
          error: string | null;
        };
        setItems((prev) =>
          prev.map((item) => {
            if (item.documentId !== data.document_id) return item;
            const nextState: UploadState =
              data.status === "done"
                ? "done"
                : data.status === "failed"
                  ? "failed"
                  : "processing";
            return {
              ...item,
              state: nextState,
              status: data.status,
              ingestPct: data.progress_pct,
              error: data.error,
            };
          }),
        );
      } catch {
        // ignore malformed events
      }
    };

    return () => {
      es.close();
      sseRef.current = null;
    };
  }, [personaId]);

  // ── Queue processing ──────────────────────────────────────────────────────

  const processNext = useCallback(() => {
    while (activeCount.current < MAX_CONCURRENT && queueRef.current.length > 0) {
      const item = queueRef.current.shift();
      if (!item) break;
      activeCount.current++;

      // Mark as uploading
      setItems((prev) =>
        prev.map((i) => (i.id === item.id ? { ...i, state: "uploading" } : i)),
      );

      uploadMutation.mutateAsync(
        {
          file: item.file,
          eraId: eraId || undefined,
          source: source || undefined,
          onProgress: (pct) => {
            setItems((prev) =>
              prev.map((i) =>
                i.id === item.id ? { ...i, uploadPct: pct } : i,
              ),
            );
          },
        },
        {
          onSuccess: (doc: Document) => {
            setItems((prev) =>
              prev.map((i) =>
                i.id === item.id
                  ? {
                      ...i,
                      state: "processing",
                      uploadPct: 100,
                      status: doc.status,
                      documentId: doc.id,
                    }
                  : i,
              ),
            );
            activeCount.current--;
            processNext();
          },
          onError: (err) => {
            setItems((prev) =>
              prev.map((i) =>
                i.id === item.id
                  ? {
                      ...i,
                      state: "failed",
                      error: err.message || "Upload failed",
                    }
                  : i,
              ),
            );
            activeCount.current--;
            processNext();
          },
        },
      );
    }
  }, [uploadMutation, eraId, source]);

  const enqueueFiles = useCallback(
    (files: File[]) => {
      const newItems: UploadItem[] = files.map((f) => ({
        id: crypto.randomUUID(),
        file: f,
        uploadPct: 0,
        ingestPct: null,
        state: "queued",
        status: null,
        documentId: null,
        error: null,
      }));
      setItems((prev) => [...prev, ...newItems]);
      queueRef.current.push(...newItems);
      processNext();
    },
    [processNext],
  );

  // ── Drag and drop ─────────────────────────────────────────────────────────

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);
      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) enqueueFiles(files);
    },
    [enqueueFiles],
  );

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };
  const handleDragLeave = () => setIsDragging(false);

  const handleFileInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    if (files.length > 0) enqueueFiles(files);
    e.target.value = "";
  };

  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div className="p-8 max-w-3xl mx-auto">
      <h1 className="text-xl font-semibold text-[var(--text)] mb-6">Upload Documents</h1>

      {/* Options row */}
      <div className="flex flex-wrap gap-4 mb-6">
        <div className="flex flex-col gap-1 min-w-[180px]">
          <label className="text-xs text-[var(--text-muted)]">Era (optional)</label>
          <Select
            value={eraId}
            onChange={(e) => setEraId(e.target.value)}
            className="text-sm"
          >
            <option value="">— No era —</option>
            {eras?.map((era) => (
              <option key={era.id} value={era.id}>
                {era.label}
              </option>
            ))}
          </Select>
        </div>

        <div className="flex flex-col gap-1 flex-1 min-w-[200px]">
          <label className="text-xs text-[var(--text-muted)]">Source (optional)</label>
          <input
            type="text"
            value={source}
            onChange={(e) => setSource(e.target.value)}
            placeholder="e.g. Personal diary, Letters from 2010…"
            className="px-3 py-1.5 text-sm border border-[var(--border)] rounded bg-[var(--bg)] text-[var(--text)] outline-none focus:ring-1 focus:ring-[var(--accent)]"
          />
        </div>
      </div>

      {/* Dropzone */}
      <label
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        className={`flex flex-col items-center justify-center border-2 border-dashed rounded-lg p-12 mb-6 cursor-pointer transition-colors ${
          isDragging
            ? "border-[var(--accent)] bg-[var(--bg-subtle)]"
            : "border-[var(--border)] hover:border-[var(--accent)] hover:bg-[var(--bg-subtle)]"
        }`}
      >
        <input
          type="file"
          multiple
          className="hidden"
          accept=".txt,.md,.pdf,.docx,.mp3,.wav,.m4a"
          onChange={handleFileInput}
        />
        <div className="text-[var(--text-muted)] text-center">
          <p className="text-base mb-1">Drop files here or click to browse</p>
          <p className="text-xs">
            Accepted: .txt, .md, .pdf, .docx, .mp3, .wav, .m4a
          </p>
          <p className="text-xs mt-0.5">Text: max 25 MB · Audio: max 500 MB</p>
        </div>
      </label>

      {/* Upload queue */}
      {items.length > 0 && (
        <div className="space-y-2">
          {items.map((item) => (
            <UploadRow key={item.id} item={item} />
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Upload row ───────────────────────────────────────────────────────────────

function UploadRow({ item }: { item: UploadItem }) {
  const pct =
    item.state === "uploading"
      ? item.uploadPct
      : item.state === "processing" && item.ingestPct != null
        ? item.ingestPct
        : item.state === "done"
          ? 100
          : null;

  const statusText =
    item.state === "queued"
      ? "Queued"
      : item.state === "uploading"
        ? `Uploading ${item.uploadPct}%`
        : item.state === "processing"
          ? item.status
            ? statusLabel(item.status)
            : "Processing…"
          : item.state === "done"
            ? "Done"
            : item.error ?? "Failed";

  const statusClass =
    item.state === "done"
      ? "text-green-600"
      : item.state === "failed"
        ? "text-red-600"
        : item.status
          ? statusColor(item.status)
          : "text-[var(--text-muted)]";

  return (
    <div className="flex items-center gap-3 p-3 rounded border border-[var(--border)] bg-[var(--bg-elevated)]">
      <div className="flex-1 min-w-0">
        <p className="text-sm text-[var(--text)] truncate">{item.file.name}</p>
        <p className="text-xs mt-0.5 truncate">
          <span className={statusClass}>{statusText}</span>
          {item.file.size > 0 && (
            <span className="text-[var(--text-subtle)] ml-2">
              {formatBytes(item.file.size)}
            </span>
          )}
          {item.state === "processing" && item.status === "transcribing" && item.ingestPct != null && (
            <span className="text-[var(--text-muted)] ml-2">
              {item.ingestPct}%
            </span>
          )}
        </p>
      </div>

      {pct != null && item.state !== "done" && item.state !== "failed" && (
        <div className="w-24 flex-shrink-0">
          <div className="h-1.5 rounded-full bg-[var(--bg-subtle)] overflow-hidden">
            <div
              className="h-full bg-[var(--accent)] transition-all duration-300"
              style={{ width: `${pct}%` }}
            />
          </div>
        </div>
      )}

      {item.state === "done" && (
        <span className="text-xs text-green-600">✓</span>
      )}
      {item.state === "failed" && (
        <span className="text-xs text-red-600">✗</span>
      )}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
