import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useErasList } from "@/lib/personas";
import {
  Document,
  DocumentKind,
  DocumentStatus,
  formatDuration,
  statusColor,
  statusLabel,
  useDeleteDocument,
  useDocumentsList,
  useReingestDocument,
} from "@/lib/documents";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Dialog, DialogFooter } from "@/components/ui/dialog";
import { Upload, RefreshCw, Trash2, FileText } from "lucide-react";

// ─── Filters ──────────────────────────────────────────────────────────────────

interface Filters {
  era_id?: string;
  kind?: DocumentKind;
  status?: DocumentStatus[];
}

const STATUS_GROUPS: { label: string; statuses: DocumentStatus[] | undefined }[] =
  [
    { label: "All", statuses: undefined },
    { label: "Done", statuses: ["done"] },
    { label: "Failed", statuses: ["failed"] },
    {
      label: "Processing",
      statuses: ["pending", "parsing", "transcribing", "chunking", "embedding", "analysing"],
    },
  ];

// ─── Component ────────────────────────────────────────────────────────────────

export default function Documents() {
  const { id: personaId } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data: eras } = useErasList(personaId!);

  const [filters, setFilters] = useState<Filters>({});
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const [allItems, setAllItems] = useState<Document[]>([]);
  const [transcriptDocId, setTranscriptDocId] = useState<string | null>(null);
  const [transcriptContent, setTranscriptContent] = useState<string | null>(null);
  const [transcriptLoading, setTranscriptLoading] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<Document | null>(null);

  const deleteDoc = useDeleteDocument(personaId!);
  const reingestDoc = useReingestDocument(personaId!);

  const { data, isLoading, isFetching } = useDocumentsList(personaId!, {
    ...filters,
    cursor,
    limit: 50,
  });

  // Merge newly loaded items
  const displayItems = cursor
    ? [...allItems, ...(data?.items ?? [])]
    : (data?.items ?? []);

  const loadMore = () => {
    if (data?.next_cursor) {
      setAllItems(displayItems);
      setCursor(data.next_cursor);
    }
  };

  const applyFilters = (next: Filters) => {
    setFilters(next);
    setCursor(undefined);
    setAllItems([]);
  };

  // ── Transcript modal ─────────────────────────────────────────────────────

  const openTranscript = async (docId: string) => {
    setTranscriptDocId(docId);
    setTranscriptLoading(true);
    setTranscriptContent(null);
    try {
      const res = await fetch(
        `/api/personas/${personaId}/documents/${docId}/transcript`,
        { credentials: "include" },
      );
      if (res.ok) {
        setTranscriptContent(await res.text());
      } else {
        setTranscriptContent("Failed to load transcript.");
      }
    } catch {
      setTranscriptContent("Network error.");
    } finally {
      setTranscriptLoading(false);
    }
  };

  // ─────────────────────────────────────────────────────────────────────────

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold text-[var(--text)]">Documents</h1>
        <Button
          size="sm"
          onClick={() => navigate(`/personas/${personaId}/upload`)}
          className="gap-2"
        >
          <Upload size={14} />
          Upload
        </Button>
      </div>

      {/* Filters */}
      <div className="flex flex-wrap gap-3 mb-5">
        {/* Era filter */}
        <Select
          value={filters.era_id ?? ""}
          onChange={(e) =>
            applyFilters({ ...filters, era_id: e.target.value || undefined })
          }
          className="text-sm"
        >
          <option value="">All eras</option>
          {eras?.map((era) => (
            <option key={era.id} value={era.id}>
              {era.label}
            </option>
          ))}
        </Select>

        {/* Kind filter */}
        <Select
          value={filters.kind ?? ""}
          onChange={(e) =>
            applyFilters({
              ...filters,
              kind: (e.target.value as DocumentKind) || undefined,
            })
          }
          className="text-sm"
        >
          <option value="">All types</option>
          <option value="text">Text</option>
          <option value="audio">Audio</option>
        </Select>

        {/* Status filter */}
        <Select
          value={
            filters.status
              ? STATUS_GROUPS.find(
                  (g) =>
                    g.statuses &&
                    JSON.stringify(g.statuses) ===
                      JSON.stringify(filters.status),
                )?.label ?? "All"
              : "All"
          }
          onChange={(e) => {
            const group = STATUS_GROUPS.find((g) => g.label === e.target.value);
            applyFilters({ ...filters, status: group?.statuses });
          }}
          className="text-sm"
        >
          {STATUS_GROUPS.map((g) => (
            <option key={g.label}>{g.label}</option>
          ))}
        </Select>
      </div>

      {/* Table */}
      {isLoading ? (
        <p className="text-sm text-[var(--text-muted)] py-8 text-center">Loading…</p>
      ) : displayItems.length === 0 ? (
        <div className="py-16 text-center text-[var(--text-muted)]">
          <p className="text-sm mb-3">No documents yet.</p>
          <Button
            size="sm"
            variant="outline"
            onClick={() => navigate(`/personas/${personaId}/upload`)}
          >
            Upload your first document
          </Button>
        </div>
      ) : (
        <>
          <div className="overflow-x-auto rounded border border-[var(--border)]">
            <table className="w-full text-sm">
              <thead className="bg-[var(--bg-subtle)] border-b border-[var(--border)]">
                <tr>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Title</th>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Kind</th>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Era</th>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Size</th>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Status</th>
                  <th className="px-4 py-2.5 text-left font-medium text-[var(--text-muted)]">Uploaded</th>
                  <th className="px-4 py-2.5 text-right font-medium text-[var(--text-muted)]">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border)]">
                {displayItems.map((doc) => (
                  <DocumentRow
                    key={doc.id}
                    doc={doc}
                    eras={eras}
                    onTranscript={openTranscript}
                    onReingest={(id) => reingestDoc.mutate(id)}
                    onDelete={(d) => setDeleteConfirm(d)}
                  />
                ))}
              </tbody>
            </table>
          </div>

          {data?.next_cursor && (
            <div className="mt-4 text-center">
              <Button
                variant="outline"
                size="sm"
                onClick={loadMore}
                disabled={isFetching}
              >
                {isFetching ? "Loading…" : "Load more"}
              </Button>
            </div>
          )}
        </>
      )}

      {/* Transcript modal */}
      {transcriptDocId && (
        <Dialog
          title="Transcript"
          open={true}
          onClose={() => {
            setTranscriptDocId(null);
            setTranscriptContent(null);
          }}
        >
          {transcriptLoading ? (
            <p className="text-sm text-[var(--text-muted)] py-4">Loading…</p>
          ) : (
            <pre className="whitespace-pre-wrap text-sm text-[var(--text)] max-h-[60vh] overflow-y-auto font-mono bg-[var(--bg-subtle)] p-4 rounded">
              {transcriptContent}
            </pre>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setTranscriptDocId(null);
                setTranscriptContent(null);
              }}
            >
              Close
            </Button>
          </DialogFooter>
        </Dialog>
      )}

      {/* Delete confirmation */}
      {deleteConfirm && (
        <Dialog
          title="Delete document?"
          open={true}
          onClose={() => setDeleteConfirm(null)}
        >
          <p className="text-sm text-[var(--text-muted)]">
            This will permanently delete{" "}
            <strong>{deleteConfirm.title ?? "this document"}</strong>, its
            transcript, and all embeddings. This cannot be undone.
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteConfirm(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                deleteDoc.mutate(deleteConfirm.id, {
                  onSettled: () => setDeleteConfirm(null),
                });
              }}
              disabled={deleteDoc.isPending}
            >
              {deleteDoc.isPending ? "Deleting…" : "Delete"}
            </Button>
          </DialogFooter>
        </Dialog>
      )}
    </div>
  );
}

// ─── Document row ─────────────────────────────────────────────────────────────

function DocumentRow({
  doc,
  eras,
  onTranscript,
  onReingest,
  onDelete,
}: {
  doc: Document;
  eras: { id: string; label: string }[] | undefined;
  onTranscript: (id: string) => void;
  onReingest: (id: string) => void;
  onDelete: (doc: Document) => void;
}) {
  const eraLabel = doc.era_id
    ? (eras?.find((e) => e.id === doc.era_id)?.label ?? "—")
    : "—";

  const sizeInfo =
    doc.kind === "audio" && doc.duration_sec != null
      ? formatDuration(doc.duration_sec)
      : doc.word_count != null
        ? `${doc.word_count.toLocaleString()} words`
        : "—";

  return (
    <tr className="hover:bg-[var(--bg-subtle)] transition-colors">
      <td className="px-4 py-2.5 max-w-[220px]">
        <span
          className="text-[var(--text)] truncate block"
          title={doc.title ?? undefined}
        >
          {doc.title ?? <span className="text-[var(--text-muted)] italic">Untitled</span>}
        </span>
        {doc.source && (
          <span className="text-xs text-[var(--text-muted)] truncate block" title={doc.source}>
            {doc.source}
          </span>
        )}
      </td>
      <td className="px-4 py-2.5 text-[var(--text-muted)] capitalize">{doc.kind}</td>
      <td className="px-4 py-2.5 text-[var(--text-muted)]">{eraLabel}</td>
      <td className="px-4 py-2.5 text-[var(--text-muted)]">{sizeInfo}</td>
      <td className="px-4 py-2.5">
        <StatusBadge status={doc.status} progressPct={doc.progress_pct} />
      </td>
      <td className="px-4 py-2.5 text-[var(--text-muted)] whitespace-nowrap">
        {new Date(doc.created_at).toLocaleDateString()}
      </td>
      <td className="px-4 py-2.5">
        <div className="flex items-center justify-end gap-1">
          {doc.kind === "audio" && doc.transcript_path && (
            <button
              title="View transcript"
              onClick={() => onTranscript(doc.id)}
              className="p-1.5 rounded text-[var(--text-muted)] hover:text-[var(--text)] hover:bg-[var(--bg-subtle)] transition-colors"
            >
              <FileText size={14} />
            </button>
          )}
          <button
            title="Re-ingest"
            onClick={() => onReingest(doc.id)}
            className="p-1.5 rounded text-[var(--text-muted)] hover:text-[var(--text)] hover:bg-[var(--bg-subtle)] transition-colors"
          >
            <RefreshCw size={14} />
          </button>
          <button
            title="Delete"
            onClick={() => onDelete(doc)}
            className="p-1.5 rounded text-[var(--text-muted)] hover:text-red-600 hover:bg-[var(--bg-subtle)] transition-colors"
          >
            <Trash2 size={14} />
          </button>
        </div>
      </td>
    </tr>
  );
}

// ─── Status badge ─────────────────────────────────────────────────────────────

function StatusBadge({
  status,
  progressPct,
}: {
  status: DocumentStatus;
  progressPct: number | null;
}) {
  const dotColor =
    status === "done"
      ? "bg-green-500"
      : status === "failed"
        ? "bg-red-500"
        : status === "pending"
          ? "bg-zinc-400"
          : "bg-amber-400";

  return (
    <span className="flex items-center gap-1.5">
      <span className={`inline-block w-1.5 h-1.5 rounded-full ${dotColor}`} />
      <span className={`text-xs ${statusColor(status)}`}>
        {statusLabel(status)}
        {status === "transcribing" && progressPct != null && ` ${progressPct}%`}
      </span>
    </span>
  );
}
