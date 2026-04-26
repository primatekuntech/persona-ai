import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { usePersona, usePatchPersona, useDeletePersona, useUploadAvatar } from "@/lib/personas";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select } from "@/components/ui/select";
import { Dialog, DialogFooter } from "@/components/ui/dialog";
import { Upload } from "lucide-react";
import toast from "react-hot-toast";

const schema = z.object({
  name: z.string().min(1, "Required").max(80, "Max 80 characters"),
  relation: z.enum(["self", "family", "friend", "other", ""]).optional(),
  description: z.string().max(2000).optional(),
  birth_year: z
    .string()
    .optional()
    .refine(
      (v) => !v || (Number(v) >= 1900 && Number(v) <= new Date().getFullYear()),
      { message: `Must be between 1900 and ${new Date().getFullYear()}.` },
    ),
});

type SettingsForm = z.infer<typeof schema>;

export default function PersonaSettings() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data: persona, isLoading } = usePersona(id!);
  const patch = usePatchPersona(id!);
  const deletePersona = useDeletePersona();
  const uploadAvatar = useUploadAvatar(id!);

  const [confirmName, setConfirmName] = useState("");
  const [showDelete, setShowDelete] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isDirty, isSubmitting },
  } = useForm<SettingsForm>({ resolver: zodResolver(schema) });

  useEffect(() => {
    if (persona) {
      reset({
        name: persona.name,
        relation: (persona.relation as SettingsForm["relation"]) ?? "",
        description: persona.description ?? "",
        birth_year: persona.birth_year?.toString() ?? "",
      });
    }
  }, [persona, reset]);

  async function onSubmit(data: SettingsForm) {
    await patch.mutateAsync({
      name: data.name,
      relation: data.relation || null,
      description: data.description || null,
      birth_year: data.birth_year ? Number(data.birth_year) : null,
    });
    reset(data);
  }

  function handleAvatarChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    uploadAvatar.mutate(file);
    e.target.value = "";
  }

  async function handleDeleteConfirm() {
    if (confirmName !== persona?.name) {
      toast.error("Name doesn't match.");
      return;
    }
    await deletePersona.mutateAsync(id!, {
      onSuccess: () => navigate("/personas"),
    });
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-[var(--text-muted)]">
        Loading…
      </div>
    );
  }

  return (
    <div className="max-w-xl mx-auto p-8 space-y-10">
      <div>
        <h1 className="text-xl font-semibold text-[var(--text)]">Settings</h1>
        <p className="text-sm text-[var(--text-muted)] mt-0.5">Edit this persona's details.</p>
      </div>

      {/* Avatar */}
      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-[var(--text)]">Avatar</h2>
        <div className="flex items-center gap-4">
          <div className="w-16 h-16 rounded-xl overflow-hidden bg-[var(--bg-subtle)] flex items-center justify-center">
            {persona?.avatar_path ? (
              <img
                src={`/api/personas/${id}/avatar?v=${persona?.updated_at}`}
                alt="avatar"
                className="w-full h-full object-cover"
              />
            ) : (
              <span className="text-2xl font-bold text-[var(--text-subtle)] select-none">
                {persona?.name?.[0]?.toUpperCase()}
              </span>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => fileInputRef.current?.click()}
          >
            <Upload size={13} className="mr-1.5" />
            Upload avatar
          </Button>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/jpeg,image/png,image/webp"
            className="hidden"
            onChange={handleAvatarChange}
          />
        </div>
        <p className="text-xs text-[var(--text-subtle)]">
          Max 2 MB · PNG, JPEG, or WebP · Resized to 512 × 512 and stored as WebP.
        </p>
      </section>

      {/* Edit form */}
      <section>
        <h2 className="text-sm font-semibold text-[var(--text)] mb-4">Details</h2>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          <div>
            <Label htmlFor="ps-name">Name</Label>
            <Input
              id="ps-name"
              {...register("name")}
              className={errors.name ? "border-[var(--danger)]" : ""}
            />
            {errors.name && (
              <p className="text-xs text-[var(--danger)] mt-1">{errors.name.message}</p>
            )}
          </div>

          <div>
            <Label htmlFor="ps-relation">Relation</Label>
            <Select id="ps-relation" {...register("relation")}>
              <option value="">None</option>
              <option value="self">Self</option>
              <option value="family">Family</option>
              <option value="friend">Friend</option>
              <option value="other">Other</option>
            </Select>
          </div>

          <div>
            <Label htmlFor="ps-desc">Description</Label>
            <Textarea
              id="ps-desc"
              placeholder="A short description of this persona…"
              rows={3}
              {...register("description")}
            />
          </div>

          <div>
            <Label htmlFor="ps-birth">Birth year</Label>
            <Input
              id="ps-birth"
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

          <Button type="submit" disabled={!isDirty || isSubmitting}>
            Save changes
          </Button>
        </form>
      </section>

      {/* Danger zone */}
      <section className="border border-[var(--danger)]/40 rounded-lg p-5 space-y-3">
        <h2 className="text-sm font-semibold text-[var(--danger)]">Danger zone</h2>
        <p className="text-sm text-[var(--text-muted)]">
          Deleting this persona removes it along with all its eras, documents, chat sessions, and
          files. This cannot be undone.
        </p>
        <Button
          variant="destructive"
          size="sm"
          onClick={() => setShowDelete(true)}
        >
          Delete persona
        </Button>
      </section>

      {/* Confirm delete */}
      <Dialog
        open={showDelete}
        onClose={() => { setShowDelete(false); setConfirmName(""); }}
        title="Delete persona"
      >
        <div className="space-y-4">
          <p className="text-sm text-[var(--text-muted)]">
            This will permanently delete <strong>{persona?.name}</strong> and all associated data.
          </p>
          <div>
            <Label htmlFor="confirm-name">
              Type <strong>{persona?.name}</strong> to confirm
            </Label>
            <Input
              id="confirm-name"
              value={confirmName}
              onChange={(e) => setConfirmName(e.target.value)}
              placeholder={persona?.name}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => { setShowDelete(false); setConfirmName(""); }}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={handleDeleteConfirm}
            disabled={confirmName !== persona?.name || deletePersona.isPending}
          >
            Delete forever
          </Button>
        </DialogFooter>
      </Dialog>
    </div>
  );
}
