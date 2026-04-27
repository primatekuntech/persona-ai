import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogFooter } from "@/components/ui/dialog";
import { Plus, Trash2, FlaskConical, Check, X } from "lucide-react";
import toast from "react-hot-toast";
import { cn } from "@/lib/utils";

// ─── Types ────────────────────────────────────────────────────────────────────

interface ProviderConfig {
  id: string;
  service: "transcription" | "llm" | "embeddings";
  provider: string;
  priority: number;
  config: Record<string, string | number | boolean>;
  enabled: boolean;
  created_at: string;
}

interface ProvidersResponse {
  providers: ProviderConfig[];
}

// ─── API calls ────────────────────────────────────────────────────────────────

function useProviders() {
  return useQuery<ProvidersResponse>({
    queryKey: ["providers"],
    queryFn: () => api<ProvidersResponse>("/api/providers"),
  });
}

function useDeleteProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api(`/api/providers/${id}`, { method: "DELETE" }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success("Provider removed.");
    },
    onError: (e) => {
      if (e instanceof ApiError && e.status === 409) {
        toast.error("Cannot delete a local (built-in) provider.");
      } else if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Failed to delete provider.");
      }
    },
  });
}

function useToggleProvider() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api(`/api/providers/${id}`, {
        method: "PATCH",
        body: JSON.stringify({ enabled }),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (e) => {
      if (e instanceof ApiError) toast.error(e.message);
      else toast.error("Failed to update provider.");
    },
  });
}

// ─── Schemas ──────────────────────────────────────────────────────────────────

const openaiCompatSchema = z.object({
  endpoint: z.string().url("Must be a valid URL."),
  model: z.string().min(1, "Model is required."),
  api_key: z.string().min(1, "API key is required."),
});

const googleSpeechSchema = z.object({
  api_key: z.string().min(1, "API key is required."),
  region: z.string().optional(),
});

type OpenAICompatForm = z.infer<typeof openaiCompatSchema>;
type GoogleSpeechForm = z.infer<typeof googleSpeechSchema>;

// ─── Add Provider Dialogs ─────────────────────────────────────────────────────

function AddOpenAICompatDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<OpenAICompatForm>({ resolver: zodResolver(openaiCompatSchema) });

  async function onSubmit(data: OpenAICompatForm) {
    try {
      await api("/api/providers", {
        method: "POST",
        body: JSON.stringify({
          service: "llm",
          provider: "openai_compat",
          priority: 5,
          config: {
            endpoint: data.endpoint,
            model: data.model,
            api_key: data.api_key,
          },
        }),
      });
      qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success("OpenAI-compatible endpoint added.");
      reset();
      onClose();
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        toast.error("This provider is already configured.");
      } else if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Failed to add provider.");
      }
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title="Add OpenAI-compatible endpoint">
      <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="endpoint">Endpoint URL</Label>
          <Input
            id="endpoint"
            placeholder="https://api.openai.com"
            {...register("endpoint")}
          />
          {errors.endpoint && (
            <p className="text-xs text-[var(--danger)]">{errors.endpoint.message}</p>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="model">Model</Label>
          <Input id="model" placeholder="gpt-4o-mini" {...register("model")} />
          {errors.model && (
            <p className="text-xs text-[var(--danger)]">{errors.model.message}</p>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="api_key">API key</Label>
          <Input
            id="api_key"
            type="password"
            placeholder="sk-..."
            autoComplete="off"
            {...register("api_key")}
          />
          <p className="text-xs text-[var(--text-subtle)]">
            Stored encrypted — never shown in full after save.
          </p>
          {errors.api_key && (
            <p className="text-xs text-[var(--danger)]">{errors.api_key.message}</p>
          )}
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Saving…" : "Add provider"}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}

function AddGoogleSpeechDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const {
    register,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<GoogleSpeechForm>({ resolver: zodResolver(googleSpeechSchema) });

  async function onSubmit(data: GoogleSpeechForm) {
    try {
      await api("/api/providers", {
        method: "POST",
        body: JSON.stringify({
          service: "transcription",
          provider: "google_speech",
          priority: 5,
          config: {
            api_key: data.api_key,
            region: data.region || "global",
          },
        }),
      });
      qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success("Google Speech-to-Text added.");
      reset();
      onClose();
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        toast.error("This provider is already configured.");
      } else if (e instanceof ApiError) {
        toast.error(e.message);
      } else {
        toast.error("Failed to add provider.");
      }
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title="Add Google Speech-to-Text">
      <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="g_api_key">API key</Label>
          <Input
            id="g_api_key"
            type="password"
            placeholder="AIza..."
            autoComplete="off"
            {...register("api_key")}
          />
          <p className="text-xs text-[var(--text-subtle)]">
            Stored encrypted — never shown in full after save.
          </p>
          {errors.api_key && (
            <p className="text-xs text-[var(--danger)]">{errors.api_key.message}</p>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="region">Region (optional)</Label>
          <Input id="region" placeholder="global" {...register("region")} />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Saving…" : "Add provider"}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}

// ─── Provider Row ──────────────────────────────────────────────────────────────

function ProviderRow({ config }: { config: ProviderConfig }) {
  const deleteProvider = useDeleteProvider();
  const toggleProvider = useToggleProvider();
  const [testStatus, setTestStatus] = useState<"idle" | "testing" | "ok" | "fail">("idle");
  const [testError, setTestError] = useState<string>("");
  const isLocal = config.provider.startsWith("local_");

  async function handleTest() {
    setTestStatus("testing");
    setTestError("");
    try {
      const result = await api<{ ok: boolean; error?: string }>(
        `/api/providers/${config.id}/test`,
        { method: "POST" },
      );
      if (result.ok) {
        setTestStatus("ok");
      } else {
        setTestStatus("fail");
        setTestError(result.error ?? "Unknown error");
      }
    } catch (e) {
      setTestStatus("fail");
      if (e instanceof ApiError) {
        setTestError(e.message);
      } else {
        setTestError("Request failed");
      }
    }
  }

  const providerLabel: Record<string, string> = {
    local_whisper: "Whisper (local)",
    local_llama: "Qwen2.5 / SeaLLM (local)",
    local_bge: "bge-m3 (local)",
    openai_compat: "OpenAI-compatible",
    google_speech: "Google Speech-to-Text",
  };

  return (
    <div
      className={cn(
        "flex items-center justify-between py-2.5 px-3 rounded-md",
        "border border-[var(--border)] bg-[var(--bg)]",
        !config.enabled && "opacity-60",
      )}
    >
      <div className="flex items-center gap-3 min-w-0">
        <div className="flex items-center gap-1.5">
          <div
            className={cn(
              "w-2 h-2 rounded-full",
              config.enabled ? "bg-green-500" : "bg-[var(--text-subtle)]",
            )}
          />
        </div>
        <div className="min-w-0">
          <p className="text-sm font-medium text-[var(--text)] truncate">
            {providerLabel[config.provider] ?? config.provider}
          </p>
          <p className="text-xs text-[var(--text-subtle)]">
            priority {config.priority}
            {config.config.api_key_hint
              ? ` · key: ${config.config.api_key_hint}`
              : ""}
            {config.config.model ? ` · ${config.config.model}` : ""}
          </p>
        </div>
        {isLocal && (
          <span className="inline-flex items-center px-1.5 py-0.5 rounded text-xs font-medium bg-[var(--bg-subtle)] text-[var(--text-muted)] shrink-0">
            built-in
          </span>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        {/* Test button */}
        <Button
          variant="outline"
          size="sm"
          onClick={handleTest}
          disabled={testStatus === "testing"}
          title="Test connectivity"
        >
          {testStatus === "testing" ? (
            <span className="text-xs">Testing…</span>
          ) : testStatus === "ok" ? (
            <Check size={14} className="text-green-600" />
          ) : testStatus === "fail" ? (
            <X size={14} className="text-[var(--danger)]" />
          ) : (
            <FlaskConical size={14} />
          )}
        </Button>

        {/* Enable/disable toggle */}
        {!isLocal && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => toggleProvider.mutate({ id: config.id, enabled: !config.enabled })}
            disabled={toggleProvider.isPending}
          >
            {config.enabled ? "Disable" : "Enable"}
          </Button>
        )}

        {/* Delete */}
        {!isLocal && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              if (confirm("Remove this provider?")) {
                deleteProvider.mutate(config.id);
              }
            }}
            disabled={deleteProvider.isPending}
            title="Remove provider"
          >
            <Trash2 size={14} className="text-[var(--danger)]" />
          </Button>
        )}
      </div>

      {/* Test error tooltip */}
      {testStatus === "fail" && testError && (
        <div className="absolute mt-8 z-10 max-w-xs bg-[var(--bg-elevated)] border border-[var(--border)] rounded-md p-2 text-xs text-[var(--danger)] shadow-lg">
          {testError}
        </div>
      )}
    </div>
  );
}

// ─── Service Section ──────────────────────────────────────────────────────────

function ServiceSection({
  title,
  service,
  configs,
  addButton,
  collapsed,
}: {
  title: string;
  service: string;
  configs: ProviderConfig[];
  addButton?: React.ReactNode;
  collapsed?: boolean;
}) {
  const [open, setOpen] = useState(!collapsed);
  const serviceConfigs = configs.filter((c) => c.service === service);

  return (
    <Card>
      <CardHeader
        className="cursor-pointer select-none"
        onClick={() => setOpen((o) => !o)}
      >
        <div className="flex items-center justify-between">
          <CardTitle className="text-base">{title}</CardTitle>
          <span className="text-[var(--text-subtle)] text-sm">{open ? "▲" : "▼"}</span>
        </div>
      </CardHeader>
      {open && (
        <CardContent className="space-y-2">
          {serviceConfigs.length === 0 && (
            <p className="text-sm text-[var(--text-subtle)]">No providers configured.</p>
          )}
          {serviceConfigs.map((c) => (
            <ProviderRow key={c.id} config={c} />
          ))}
          {addButton && <div className="pt-1">{addButton}</div>}
        </CardContent>
      )}
    </Card>
  );
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export default function Integrations() {
  const { data, isLoading } = useProviders();
  const [addOpenAI, setAddOpenAI] = useState(false);
  const [addGoogle, setAddGoogle] = useState(false);

  const providers = data?.providers ?? [];

  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Integrations</h1>
      </div>

      <div className="flex-1 overflow-y-auto p-6 max-w-2xl space-y-4">
        {isLoading ? (
          <p className="text-sm text-[var(--text-muted)]">Loading providers…</p>
        ) : (
          <>
            {/* Transcription */}
            <ServiceSection
              title="Transcription"
              service="transcription"
              configs={providers}
              addButton={
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={() => setAddGoogle(true)}
                >
                  <Plus size={14} />
                  Add Google Speech-to-Text
                </Button>
              }
            />

            {/* Language model */}
            <ServiceSection
              title="Language model"
              service="llm"
              configs={providers}
              addButton={
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5"
                  onClick={() => setAddOpenAI(true)}
                >
                  <Plus size={14} />
                  Add OpenAI-compatible endpoint
                </Button>
              }
            />

            {/* Embeddings (collapsed by default — advanced) */}
            <ServiceSection
              title="Embeddings"
              service="embeddings"
              configs={providers}
              collapsed
            />
          </>
        )}

        <p className="text-xs text-[var(--text-subtle)] pt-2">
          Local providers run entirely on-device and are always available.
          Cloud providers are tried in priority order; lower number = tried first.
          API keys are encrypted at rest and never returned in full.
        </p>
      </div>

      <AddOpenAICompatDialog open={addOpenAI} onClose={() => setAddOpenAI(false)} />
      <AddGoogleSpeechDialog open={addGoogle} onClose={() => setAddGoogle(false)} />
    </div>
  );
}
