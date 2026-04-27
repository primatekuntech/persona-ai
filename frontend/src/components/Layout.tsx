import { Link, NavLink, Outlet } from "react-router-dom";
import { useLogout, useMe } from "@/lib/auth";
import { Button } from "@/components/ui/button";
import { Users, Mail, MessageSquare, Settings, LogOut, Puzzle } from "lucide-react";
import { cn } from "@/lib/utils";

function NavItem({
  to,
  icon: Icon,
  label,
}: {
  to: string;
  icon: React.ElementType;
  label: string;
}) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        cn(
          "flex items-center gap-2.5 px-3 py-2 rounded text-sm font-medium transition-colors",
          isActive
            ? "bg-[var(--bg-subtle)] text-[var(--text)]"
            : "text-[var(--text-muted)] hover:bg-[var(--bg-subtle)] hover:text-[var(--text)]",
        )
      }
    >
      <Icon size={16} />
      {label}
    </NavLink>
  );
}

export default function Layout() {
  const { data: me } = useMe();
  const logout = useLogout();

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--bg)]">
      {/* Sidebar */}
      <aside className="w-56 shrink-0 flex flex-col border-r border-[var(--border)] bg-[var(--bg-elevated)]">
        <div className="h-14 flex items-center px-4 border-b border-[var(--border)]">
          <Link to="/personas" className="text-base font-semibold tracking-tight text-[var(--text)]">
            Persona AI
          </Link>
        </div>

        <nav className="flex-1 overflow-y-auto p-2 space-y-0.5">
          <NavItem to="/personas" icon={MessageSquare} label="Personas" />
          <NavItem to="/settings/account" icon={Settings} label="Settings" />
          <NavItem to="/settings/integrations" icon={Puzzle} label="Integrations" />

          {me?.role === "admin" && (
            <>
              <div className="pt-4 pb-1 px-3">
                <span className="text-xs font-medium text-[var(--text-subtle)] uppercase tracking-wider">
                  Admin
                </span>
              </div>
              <NavItem to="/admin/users" icon={Users} label="Users" />
              <NavItem to="/admin/invites" icon={Mail} label="Invites" />
            </>
          )}
        </nav>

        <div className="p-2 border-t border-[var(--border)]">
          <div className="px-3 py-1.5 mb-1">
            <p className="text-sm font-medium text-[var(--text)] truncate">
              {me?.display_name ?? me?.email}
            </p>
            <p className="text-xs text-[var(--text-subtle)] truncate">{me?.email}</p>
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="w-full justify-start gap-2"
            onClick={() => logout.mutate()}
          >
            <LogOut size={14} />
            Sign out
          </Button>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
