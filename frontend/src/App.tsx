import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { useMe } from "@/lib/auth";
import Layout from "@/components/Layout";
import Login from "@/pages/Login";
import AcceptInvite from "@/pages/AcceptInvite";
import ForgotPassword from "@/pages/ForgotPassword";
import ResetPassword from "@/pages/ResetPassword";
import Personas from "@/pages/Personas";
import Settings from "@/pages/Settings";
import AdminUsers from "@/pages/admin/Users";
import AdminInvites from "@/pages/admin/Invites";

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { data, isLoading, isError } = useMe();
  if (isLoading) return <div className="flex items-center justify-center h-screen text-text-muted text-sm">Loading…</div>;
  if (isError || !data) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

function RequireAdmin({ children }: { children: React.ReactNode }) {
  const { data, isLoading, isError } = useMe();
  if (isLoading) return null;
  if (isError || !data) return <Navigate to="/login" replace />;
  if (data.role !== "admin") return <Navigate to="/personas" replace />;
  return <>{children}</>;
}

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        {/* Public */}
        <Route path="/login" element={<Login />} />
        <Route path="/accept-invite" element={<AcceptInvite />} />
        <Route path="/forgot-password" element={<ForgotPassword />} />
        <Route path="/reset-password" element={<ResetPassword />} />

        {/* Authenticated */}
        <Route
          element={
            <RequireAuth>
              <Layout />
            </RequireAuth>
          }
        >
          <Route path="/personas" element={<Personas />} />
          <Route path="/settings/account" element={<Settings />} />
          <Route
            path="/admin/users"
            element={
              <RequireAdmin>
                <AdminUsers />
              </RequireAdmin>
            }
          />
          <Route
            path="/admin/invites"
            element={
              <RequireAdmin>
                <AdminInvites />
              </RequireAdmin>
            }
          />
        </Route>

        <Route path="/" element={<Navigate to="/personas" replace />} />
        <Route path="*" element={<Navigate to="/personas" replace />} />
      </Routes>
    </BrowserRouter>
  );
}
