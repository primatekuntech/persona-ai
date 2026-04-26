import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ChevronDown, Search, User2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePersonasList } from "@/lib/personas";

interface Props {
  currentPersonaId: string;
}

export default function PersonaSwitcher({ currentPersonaId }: Props) {
  const { data } = usePersonasList();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const personas = data?.items ?? [];
  const current = personas.find((p) => p.id === currentPersonaId);
  const filtered = personas.filter((p) =>
    p.name.toLowerCase().includes(query.toLowerCase()),
  );

  // Cmd+P opens the switcher
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "p") {
        e.preventDefault();
        setOpen(true);
      }
      if (e.key === "Escape" && open) {
        setOpen(false);
        setQuery("");
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  // Close on outside click
  useEffect(() => {
    function onMouseDown(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    }
    document.addEventListener("mousedown", onMouseDown);
    return () => document.removeEventListener("mousedown", onMouseDown);
  }, []);

  // Auto-focus input when dropdown opens
  useEffect(() => {
    if (open) {
      setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [open]);

  function select(id: string) {
    setOpen(false);
    setQuery("");
    navigate(`/personas/${id}/dashboard`);
  }

  return (
    <div ref={containerRef} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className={cn(
          "flex items-center gap-2 px-3 py-1.5 rounded text-sm font-medium transition-colors max-w-[200px]",
          "text-[var(--text)] hover:bg-[var(--bg-subtle)]",
          open && "bg-[var(--bg-subtle)]",
        )}
        title="Switch persona (⌘P)"
      >
        <User2 size={14} className="shrink-0 text-[var(--text-subtle)]" />
        <span className="truncate">{current?.name ?? "Select persona"}</span>
        <ChevronDown
          size={13}
          className={cn(
            "shrink-0 text-[var(--text-subtle)] transition-transform",
            open && "rotate-180",
          )}
        />
      </button>

      {open && (
        <div className="absolute left-0 top-full mt-1 w-72 z-50 bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg shadow-lg overflow-hidden">
          <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--border)]">
            <Search size={13} className="text-[var(--text-subtle)] shrink-0" />
            <input
              ref={inputRef}
              type="text"
              placeholder="Search personas… (⌘P)"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="flex-1 bg-transparent text-sm text-[var(--text)] placeholder:text-[var(--text-subtle)] outline-none"
            />
          </div>
          <div className="max-h-60 overflow-y-auto py-1">
            {filtered.length === 0 ? (
              <p className="px-3 py-2 text-sm text-[var(--text-subtle)]">
                No personas found.
              </p>
            ) : (
              filtered.map((p) => (
                <button
                  key={p.id}
                  onClick={() => select(p.id)}
                  className={cn(
                    "w-full flex items-center gap-2 px-3 py-2 text-left text-sm transition-colors",
                    p.id === currentPersonaId
                      ? "bg-[var(--bg-subtle)] text-[var(--text)] font-medium"
                      : "text-[var(--text-muted)] hover:bg-[var(--bg-subtle)] hover:text-[var(--text)]",
                  )}
                >
                  <User2 size={13} className="shrink-0 text-[var(--text-subtle)]" />
                  <span className="truncate">{p.name}</span>
                  {p.relation && (
                    <span className="ml-auto text-xs text-[var(--text-subtle)] capitalize shrink-0">
                      {p.relation}
                    </span>
                  )}
                </button>
              ))
            )}
          </div>
          <div className="border-t border-[var(--border)] px-3 py-2">
            <button
              onClick={() => { setOpen(false); navigate("/personas"); }}
              className="text-xs text-[var(--text-subtle)] hover:text-[var(--text)] transition-colors"
            >
              All personas →
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
