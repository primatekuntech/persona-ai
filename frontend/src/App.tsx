import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { useMe } from "@/lib/auth";
import Layout from "@/components/Layout";
import PersonaWorkspaceLayout from "@/components/PersonaWorkspaceLayout";
import Login from "@/pages/Login";
import AcceptInvite from "@/pages/AcceptInvite";
import ForgotPassword from "@/pages/ForgotPassword";
import ResetPassword from "@/pages/ResetPassword";
import Personas from "@/pages/Personas";
import Settings from "@/pages/Settings";
import AdminUsers from "@/pages/admin/Users";
import AdminInvites from "@/pages/admin/Invites";
import AdminJobs from "@/pages/admin/Jobs";
import AdminErrors from "@/pages/admin/Errors";
import AdminAudit from "@/pages/admin/Audit";
import PersonaDashboard from "@/pages/personas/Dashboard";
import ErasPage from "@/pages/personas/Eras";
import PersonaSettings from "@/pages/personas/PersonaSettings";
import DocumentsPage from "@/pages/personas/Documents";
import UploadPage from "@/pages/personas/Upload";
import ChatPage from "@/pages/personas/Chat";
import Integrations from "@/pages/Integrations";

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

        {/* Persona workspace (has its own layout) */}
        <Route
          path="/personas/:id"
          element={
            <RequireAuth>
              <PersonaWorkspaceLayout />
            </RequireAuth>
          }
        >
          <Route index element={<Navigate to="dashboard" replace />} />
          <Route path="dashboard" element={<PersonaDashboard />} />
          <Route path="eras" element={<ErasPage />} />
          <Route path="settings" element={<PersonaSettings />} />
          <Route path="documents" element={<DocumentsPage />} />
          <Route path="upload" element={<UploadPage />} />
          <Route path="chat" element={<ChatPage />} />
          <Route path="chat/:sessionId" element={<ChatPage />} />
        </Route>

        {/* Authenticated with main layout */}
        <Route
          element={
            <RequireAuth>
              <Layout />
            </RequireAuth>
          }
        >
          <Route path="/personas" element={<Personas />} />
          <Route path="/settings/account" element={<Settings />} />
          <Route path="/settings/integrations" element={<Integrations />} />
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
          <Route
            path="/admin/jobs"
            element={
              <RequireAdmin>
                <AdminJobs />
              </RequireAdmin>
            }
          />
          <Route
            path="/admin/errors"
            element={
              <RequireAdmin>
                <AdminErrors />
              </RequireAdmin>
            }
          />
          <Route
            path="/admin/audit"
            element={
              <RequireAdmin>
                <AdminAudit />
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
