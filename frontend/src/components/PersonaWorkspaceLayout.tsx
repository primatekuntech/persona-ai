import { NavLink, Outlet, useNavigate, useParams } from "react-router-dom";
import { useMe, useLogout } from "@/lib/auth";
import { Button } from "@/components/ui/button";
import PersonaSwitcher from "@/components/PersonaSwitcher";
import {
  ArrowLeft,
  BookOpen,
  Calendar,
  LayoutDashboard,
  LogOut,
  MessageSquare,
  Settings,
  Upload,
} from "lucide-react";
import { cn } from "@/lib/utils";

function SubNavItem({
  to,
  icon: Icon,
  label,
  disabled,
}: {
  to: string;
  icon: React.ElementType;
  label: string;
  disabled?: boolean;
}) {
  if (disabled) {
    return (
      <div
        className="flex items-center gap-2.5 px-3 py-2 rounded text-sm font-medium opacity-40 cursor-not-allowed text-[var(--text-muted)]"
        title="Coming in a later sprint"
      >
        <Icon size={15} />
        {label}
      </div>
    );
  }
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
      <Icon size={15} />
      {label}
    </NavLink>
  );
}

export default function PersonaWorkspaceLayout() {
  const { id } = useParams<{ id: string }>();
  const { data: me } = useMe();
  const logout = useLogout();
  const navigate = useNavigate();

  const base = `/personas/${id}`;

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--bg)]">
      {/* Sub-sidebar */}
      <aside className="w-56 shrink-0 flex flex-col border-r border-[var(--border)] bg-[var(--bg-elevated)]">
        {/* Top: switcher */}
        <div className="h-14 flex items-center px-3 border-b border-[var(--border)] gap-1">
          <button
            onClick={() => navigate("/personas")}
            className="p-1 rounded text-[var(--text-subtle)] hover:text-[var(--text)] hover:bg-[var(--bg-subtle)] transition-colors"
            title="All personas"
          >
            <ArrowLeft size={14} />
          </button>
          <PersonaSwitcher currentPersonaId={id!} />
        </div>

        {/* Nav */}
        <nav className="flex-1 overflow-y-auto p-2 space-y-0.5">
          <SubNavItem to={`${base}/dashboard`} icon={LayoutDashboard} label="Dashboard" />
          <SubNavItem to={`${base}/documents`} icon={BookOpen} label="Documents" />
          <SubNavItem to={`${base}/upload`} icon={Upload} label="Upload" />
          <SubNavItem to={`${base}/chat`} icon={MessageSquare} label="Chat" />
          <SubNavItem to={`${base}/eras`} icon={Calendar} label="Eras" />
          <SubNavItem to={`${base}/settings`} icon={Settings} label="Settings" />
        </nav>

        {/* Footer */}
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
