import { useParams } from "react-router-dom";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import {
  useErasList,
  useCreateEra,
  usePatchEra,
  useDeleteEra,
  usePersona,
  Era,
} from "@/lib/personas";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogFooter } from "@/components/ui/dialog";
import { Pencil, Plus, Trash2 } from "lucide-react";

// ─── Schema ───────────────────────────────────────────────────────────────────

const eraSchema = z
  .object({
    label: z.string().min(1, "Required").max(40, "Max 40 characters"),
    start_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Use YYYY-MM-DD").or(z.literal("")),
    end_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Use YYYY-MM-DD").or(z.literal("")),
    description: z.string().max(500).optional(),
  })
  .refine(
    (d) => {
      if (d.start_date && d.end_date) return d.end_date >= d.start_date;
      return true;
    },
    { message: "End date must not be before start date.", path: ["end_date"] },
  );

type EraForm = z.infer<typeof eraSchema>;

// ─── Era form dialog ──────────────────────────────────────────────────────────

function EraFormDialog({
  open,
  onClose,
  personaId,
  persona,
  existing,
}: {
  open: boolean;
  onClose: () => void;
  personaId: string;
  persona: { birth_year: number | null } | undefined;
  existing?: Era;
}) {
  const createEra = useCreateEra(personaId);
  const patchEra = usePatchEra(personaId);

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    formState: { errors, isSubmitting },
  } = useForm<EraForm>({
    resolver: zodResolver(eraSchema),
    defaultValues: {
      label: existing?.label ?? "",
      start_date: existing?.start_date ?? "",
      end_date: existing?.end_date ?? "",
      description: existing?.description ?? "",
    },
  });

  const [ageStart, setAgeStart] = useState("");
  const [ageEnd, setAgeEnd] = useState("");

  function applyAgeRange() {
    const birth = persona?.birth_year;
    if (!birth) return;
    const a = parseInt(ageStart);
    const b = parseInt(ageEnd);
    if (Number.isInteger(a)) setValue("start_date", `${birth + a}-01-01`);
    if (Number.isInteger(b)) setValue("end_date", `${birth + b}-12-31`);
    if (Number.isInteger(a) && Number.isInteger(b)) {
      setValue("label", `age ${a}–${b}`);
    }
  }

  async function onSubmit(data: EraForm) {
    const payload = {
      label: data.label,
      start_date: data.start_date || null,
      end_date: data.end_date || null,
      description: data.description || null,
    };
    try {
      if (existing) {
        await patchEra.mutateAsync({ eraId: existing.id, body: payload });
      } else {
        await createEra.mutateAsync(payload);
      }
      reset();
      onClose();
    } catch {
      // error toasts handled in the mutation
    }
  }

  return (
    <Dialog
      open={open}
      onClose={() => { reset(); onClose(); }}
      title={existing ? "Edit era" : "Add era"}
    >
      <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
        <div>
          <Label htmlFor="era-label">Label</Label>
          <Input
            id="era-label"
            placeholder="e.g. age 13–16 or 2010–2012"
            {...register("label")}
            className={errors.label ? "border-[var(--danger)]" : ""}
          />
          {errors.label && (
            <p className="text-xs text-[var(--danger)] mt-1">{errors.label.message}</p>
          )}
        </div>

        {persona?.birth_year && (
          <div className="rounded-lg border border-[var(--border)] p-3 space-y-2">
            <p className="text-xs font-medium text-[var(--text-subtle)]">
              Age range helper (birth year: {persona.birth_year})
            </p>
            <div className="flex items-center gap-2">
              <Input
                placeholder="From age"
                value={ageStart}
                onChange={(e) => setAgeStart(e.target.value)}
                className="flex-1 h-7 text-xs"
                type="number"
                min={0}
                max={120}
              />
              <span className="text-[var(--text-subtle)] text-sm">–</span>
              <Input
                placeholder="To age"
                value={ageEnd}
                onChange={(e) => setAgeEnd(e.target.value)}
                className="flex-1 h-7 text-xs"
                type="number"
                min={0}
                max={120}
              />
              <Button type="button" variant="outline" size="sm" onClick={applyAgeRange}>
                Apply
              </Button>
            </div>
          </div>
        )}

        <div className="grid grid-cols-2 gap-3">
          <div>
            <Label htmlFor="era-start">Start date</Label>
            <Input
              id="era-start"
              type="date"
              {...register("start_date")}
              className={errors.start_date ? "border-[var(--danger)]" : ""}
            />
            {errors.start_date && (
              <p className="text-xs text-[var(--danger)] mt-1">{errors.start_date.message}</p>
            )}
          </div>
          <div>
            <Label htmlFor="era-end">End date</Label>
            <Input
              id="era-end"
              type="date"
              {...register("end_date")}
              className={errors.end_date ? "border-[var(--danger)]" : ""}
            />
            {errors.end_date && (
              <p className="text-xs text-[var(--danger)] mt-1">{errors.end_date.message}</p>
            )}
          </div>
        </div>

        <div>
          <Label htmlFor="era-desc">Description (optional)</Label>
          <Textarea
            id="era-desc"
            placeholder="A short description of this period…"
            rows={2}
            {...register("description")}
          />
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => { reset(); onClose(); }}>
            Cancel
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {existing ? "Save changes" : "Add era"}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export default function ErasPage() {
  const { id: personaId } = useParams<{ id: string }>();
  const { data: persona } = usePersona(personaId!);
  const { data: eras = [], isLoading } = useErasList(personaId!);
  const deleteEra = useDeleteEra(personaId!);

  const [showAdd, setShowAdd] = useState(false);
  const [editing, setEditing] = useState<Era | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Era | null>(null);

  function handleDelete(era: Era) {
    deleteEra.mutate(era.id, {
      onSuccess: () => setConfirmDelete(null),
    });
  }

  return (
    <div className="max-w-3xl mx-auto p-8">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-semibold text-[var(--text)]">Eras</h1>
          <p className="text-sm text-[var(--text-muted)] mt-0.5">
            Organise this persona's life into time periods.
          </p>
        </div>
        <Button size="sm" onClick={() => setShowAdd(true)}>
          <Plus size={14} className="mr-1" />
          Add era
        </Button>
      </div>

      {isLoading && (
        <p className="text-sm text-[var(--text-muted)]">Loading…</p>
      )}

      {!isLoading && eras.length === 0 && (
        <div className="text-center py-12 border border-dashed border-[var(--border)] rounded-lg">
          <p className="text-sm text-[var(--text-muted)]">No eras yet.</p>
          <Button
            size="sm"
            variant="ghost"
            className="mt-3"
            onClick={() => setShowAdd(true)}
          >
            <Plus size={14} className="mr-1" />
            Add the first era
          </Button>
        </div>
      )}

      {eras.length > 0 && (
        <div className="border border-[var(--border)] rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-[var(--bg-subtle)]">
              <tr>
                <th className="text-left px-4 py-2.5 text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wide">
                  Label
                </th>
                <th className="text-left px-4 py-2.5 text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wide">
                  Start
                </th>
                <th className="text-left px-4 py-2.5 text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wide">
                  End
                </th>
                <th className="text-left px-4 py-2.5 text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wide">
                  Description
                </th>
                <th className="px-4 py-2.5" />
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--border)]">
              {eras.map((era) => (
                <tr key={era.id} className="hover:bg-[var(--bg-subtle)] transition-colors">
                  <td className="px-4 py-3 font-medium text-[var(--text)]">{era.label}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)]">{era.start_date ?? "—"}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)]">{era.end_date ?? "—"}</td>
                  <td className="px-4 py-3 text-[var(--text-muted)] max-w-[200px] truncate">
                    {era.description ?? "—"}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-1 justify-end">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setEditing(era)}
                        className="h-7 w-7 p-0"
                      >
                        <Pencil size={13} />
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setConfirmDelete(era)}
                        className="h-7 w-7 p-0 text-[var(--danger)] hover:text-[var(--danger)]"
                      >
                        <Trash2 size={13} />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Add / Edit dialog */}
      <EraFormDialog
        open={showAdd}
        onClose={() => setShowAdd(false)}
        personaId={personaId!}
        persona={persona}
      />
      {editing && (
        <EraFormDialog
          open
          onClose={() => setEditing(null)}
          personaId={personaId!}
          persona={persona}
          existing={editing}
        />
      )}

      {/* Confirm delete dialog */}
      <Dialog
        open={Boolean(confirmDelete)}
        onClose={() => setConfirmDelete(null)}
        title="Delete era"
      >
        <p className="text-sm text-[var(--text-muted)]">
          Delete era <strong>{confirmDelete?.label}</strong>? Documents in this era will remain but
          will no longer be associated with an era.
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={() => setConfirmDelete(null)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => confirmDelete && handleDelete(confirmDelete)}
            disabled={deleteEra.isPending}
          >
            Delete era
          </Button>
        </DialogFooter>
      </Dialog>
    </div>
  );
}
