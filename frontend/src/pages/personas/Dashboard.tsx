import { useParams } from "react-router-dom";
import { usePersona, useErasList } from "@/lib/personas";
import { RelationBadge } from "@/components/ui/badge";
import { BookOpen, Calendar, CheckCircle2, Circle } from "lucide-react";

function CheckItem({
  done,
  label,
  sub,
  disabled,
}: {
  done: boolean;
  label: string;
  sub?: string;
  disabled?: boolean;
}) {
  return (
    <div
      className={`flex items-start gap-3 py-2 ${disabled ? "opacity-50" : ""}`}
    >
      {done ? (
        <CheckCircle2 size={17} className="text-[var(--success)] mt-0.5 shrink-0" />
      ) : (
        <Circle size={17} className="text-[var(--text-subtle)] mt-0.5 shrink-0" />
      )}
      <div>
        <p className={`text-sm font-medium ${done ? "line-through text-[var(--text-subtle)]" : "text-[var(--text)]"}`}>
          {label}
        </p>
        {sub && <p className="text-xs text-[var(--text-subtle)] mt-0.5">{sub}</p>}
      </div>
    </div>
  );
}

export default function PersonaDashboard() {
  const { id } = useParams<{ id: string }>();
  const { data: persona, isLoading } = usePersona(id!);
  const { data: eras = [] } = useErasList(id!);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-muted)]">
        Loading…
      </div>
    );
  }

  if (!persona) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-muted)]">
        Persona not found.
      </div>
    );
  }

  return (
    <div className="max-w-2xl mx-auto p-8">
      {/* Header */}
      <div className="mb-8">
        <div className="flex items-center gap-3 mb-2">
          <h1 className="text-2xl font-bold text-[var(--text)]">{persona.name}</h1>
          <RelationBadge relation={persona.relation} />
        </div>
        {persona.description && (
          <p className="text-sm text-[var(--text-muted)] mt-1">{persona.description}</p>
        )}
        {persona.birth_year && (
          <p className="text-xs text-[var(--text-subtle)] mt-1">Born {persona.birth_year}</p>
        )}
      </div>

      {/* Getting started */}
      <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-5">
        <h2 className="text-sm font-semibold text-[var(--text)] mb-3">Getting started</h2>
        <div className="divide-y divide-[var(--border)]">
          <CheckItem
            done={eras.length > 0}
            label="Create at least one era (optional)"
            sub="Eras let you organise documents by life period."
          />
          <CheckItem
            done={false}
            label="Upload documents"
            sub="Add text files, journals, or audio recordings."
            disabled
          />
          <CheckItem
            done={false}
            label="Generate style profile"
            sub="Auto-built once you have enough documents."
            disabled
          />
          <CheckItem
            done={false}
            label="Start a chat"
            sub="Converse with this persona's writing voice."
            disabled
          />
        </div>
      </div>

      {/* Quick stats */}
      <div className="mt-6 grid grid-cols-2 gap-4">
        <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-4 flex items-center gap-3">
          <BookOpen size={18} className="text-[var(--text-subtle)]" />
          <div>
            <p className="text-lg font-semibold text-[var(--text)]">0</p>
            <p className="text-xs text-[var(--text-subtle)]">Documents</p>
          </div>
        </div>
        <div className="bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-4 flex items-center gap-3">
          <Calendar size={18} className="text-[var(--text-subtle)]" />
          <div>
            <p className="text-lg font-semibold text-[var(--text)]">{eras.length}</p>
            <p className="text-xs text-[var(--text-subtle)]">Eras</p>
          </div>
        </div>
      </div>
    </div>
  );
}
