import { Button } from "@/components/ui/button";
import { MessageSquare } from "lucide-react";

export default function Personas() {
  return (
    <div className="flex flex-col h-full">
      <div className="h-14 flex items-center px-6 border-b border-[var(--border)]">
        <h1 className="text-lg font-semibold text-[var(--text)]">Personas</h1>
      </div>

      <div className="flex-1 flex flex-col items-center justify-center gap-4 text-center px-4">
        <div className="w-12 h-12 rounded-xl bg-[var(--bg-subtle)] flex items-center justify-center">
          <MessageSquare size={24} className="text-[var(--text-subtle)]" />
        </div>
        <div>
          <h2 className="text-xl font-semibold text-[var(--text)]">No personas yet</h2>
          <p className="text-sm text-[var(--text-muted)] mt-1 max-w-xs">
            Create a persona to get started. Each persona represents a writing voice with its own documents and style.
          </p>
        </div>
        <Button disabled title="Coming in the next sprint">
          Create persona
        </Button>
      </div>
    </div>
  );
}
