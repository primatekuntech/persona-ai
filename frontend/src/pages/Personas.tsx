import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import {
  usePersonasList,
  useCreatePersona,
  PersonaWithCounts,
} from "@/lib/personas";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select } from "@/components/ui/select";
import { Dialog, DialogFooter } from "@/components/ui/dialog";
import { RelationBadge } from "@/components/ui/badge";
import { BookOpen, Calendar, Plus, User2 } from "lucide-react";
import { formatDistanceToNow } from "@/lib/utils";

// ─── Create dialog ────────────────────────────────────────────────────────────

const createSchema = z.object({
  name: z.string().min(1, "Required").max(80, "Max 80 characters"),
  relation: z.enum(["self", "family", "friend", "other", ""]).optional(),
  description: z.string().max(2000).optional(),
  birth_year: z
    .string()
    .optional()
    .refine(
      (v) => !v || (Number(v) >= 1900 && Number(v) <= new Date().getFullYear()),
      { message: `Must be 1900–${new Date().getFullYear()}.` },
    ),
});
type CreateForm = z.infer<typeof createSchema>;

function CreatePersonaDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const createPersona = useCreatePersona();

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<CreateForm>({ resolver: zodResolver(createSchema) });

  async function onSubmit(data: CreateForm) {
    const persona = await createPersona.mutateAsync({
      name: data.name,
      relation: data.relation || null,
      description: data.description || null,
      birth_year: data.birth_year ? Number(data.birth_year) : null,
    });
    reset();
    onClose();
    navigate(`/personas/${persona.id}/dashboard`);
  }

  return (
    <Dialog
      open={open}
      onClose={() => { reset(); onClose(); }}
      title="Create persona"
    >
      <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
        <div>
          <Label htmlFor="cp-name">Name</Label>
          <Input
            id="cp-name"
            autoFocus
            placeholder="e.g. Me, age 17"
            {...register("name")}
            className={errors.name ? "border-[var(--danger)]" : ""}
          />
          {errors.name && (
            <p className="text-xs text-[var(--danger)] mt-1">{errors.name.message}</p>
          )}
        </div>

        <div>
          <Label htmlFor="cp-relation">Relation</Label>
          <Select id="cp-relation" {...register("relation")}>
            <option value="">None</option>
            <option value="self">Self</option>
            <option value="family">Family</option>
            <option value="friend">Friend</option>
            <option value="other">Other</option>
          </Select>
        </div>

        <div>
          <Label htmlFor="cp-desc">Description (optional)</Label>
          <Textarea
            id="cp-desc"
            placeholder="A short note about who this persona is…"
            rows={2}
            {...register("description")}
          />
        </div>

        <div>
          <Label htmlFor="cp-birth">Birth year (optional)</Label>
          <Input
            id="cp-birth"
            type="number"
            min={1900}
            max={new Date().getFullYear()}
            placeholder="e.g. 1985"
            {...register("birth_year")}
            className={errors.birth_year ? "border-[var(--danger)]" : ""}
          />
          {errors.birth_year && (
            <p className="text-xs text-[var(--danger)] mt-1">{errors.birth_year.message}</p>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => { reset(); onClose(); }}>
            Cancel
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            Create persona
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}

// ─── Persona card ─────────────────────────────────────────────────────────────

function PersonaCard({ persona }: { persona: PersonaWithCounts }) {
  const navigate = useNavigate();
  return (
    <button
      onClick={() => navigate(`/personas/${persona.id}/dashboard`)}
      className="w-full text-left bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg p-4 hover:border-[var(--text-subtle)] transition-colors space-y-3"
    >
      <div className="flex items-start gap-3">
        <div className="w-10 h-10 rounded-lg bg-[var(--bg-subtle)] flex items-center justify-center shrink-0 overflow-hidden">
          {persona.avatar_path ? (
            <img
              src={`/api/personas/${persona.id}/avatar?v=${persona.updated_at}`}
              alt="avatar"
              className="w-full h-full object-cover"
            />
          ) : (
            <User2 size={18} className="text-[var(--text-subtle)]" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-semibold text-sm text-[var(--text)] truncate">
              {persona.name}
            </span>
            <RelationBadge relation={persona.relation} />
          </div>
          {persona.description && (
            <p className="text-xs text-[var(--text-muted)] mt-0.5 line-clamp-2">
              {persona.description}
            </p>
          )}
        </div>
      </div>
      <div className="flex items-center gap-4 text-xs text-[var(--text-subtle)]">
        <span className="flex items-center gap-1">
          <BookOpen size={11} />
          {persona.doc_count} doc{persona.doc_count !== 1 ? "s" : ""}
        </span>
        <span className="flex items-center gap-1">
          <Calendar size={11} />
          {persona.era_count} era{persona.era_count !== 1 ? "s" : ""}
        </span>
        <span className="ml-auto">
          {formatDistanceToNow(new Date(persona.created_at))}
        </span>
      </div>
    </button>
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export default function Personas() {
  const { data, isLoading } = usePersonasList();
  const [showCreate, setShowCreate] = useState(false);

  const personas = data?.items ?? [];

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="h-14 flex items-center justify-between px-6 border-b border-[var(--border)] shrink-0">
        <h1 className="text-base font-semibold text-[var(--text)]">Personas</h1>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus size={14} className="mr-1" />
          Create persona
        </Button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
        {isLoading && (
          <p className="text-sm text-[var(--text-muted)]">Loading…</p>
        )}

        {!isLoading && personas.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-4 text-center py-20">
            <div className="w-12 h-12 rounded-xl bg-[var(--bg-subtle)] flex items-center justify-center">
              <User2 size={24} className="text-[var(--text-subtle)]" />
            </div>
            <div>
              <h2 className="text-lg font-semibold text-[var(--text)]">No personas yet</h2>
              <p className="text-sm text-[var(--text-muted)] mt-1 max-w-xs">
                Create a persona to get started. Each persona represents a writing voice with its own
                documents and style.
              </p>
            </div>
            <Button onClick={() => setShowCreate(true)}>
              <Plus size={14} className="mr-1" />
              Create your first persona
            </Button>
          </div>
        )}

        {personas.length > 0 && (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 max-w-5xl">
            {personas.map((p) => (
              <PersonaCard key={p.id} persona={p} />
            ))}
          </div>
        )}
      </div>

      <CreatePersonaDialog open={showCreate} onClose={() => setShowCreate(false)} />
    </div>
  );
}
