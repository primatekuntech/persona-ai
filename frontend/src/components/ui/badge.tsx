import { cn } from "@/lib/utils";

const RELATION_COLORS: Record<string, string> = {
  self: "bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-300",
  family: "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-300",
  friend: "bg-purple-100 text-purple-800 dark:bg-purple-900/30 dark:text-purple-300",
  other: "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
};

export function RelationBadge({ relation }: { relation: string | null }) {
  if (!relation) return null;
  const color = RELATION_COLORS[relation] ?? RELATION_COLORS.other;
  return (
    <span
      className={cn(
        "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium capitalize",
        color,
      )}
    >
      {relation}
    </span>
  );
}
