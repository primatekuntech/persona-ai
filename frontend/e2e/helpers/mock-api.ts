import type { Page, Route } from "@playwright/test";

// ─── Fixture data ─────────────────────────────────────────────────────────────

export const MOCK_USER = {
  id: "00000000-0000-7000-8000-000000000001",
  email: "test@example.com",
  role: "user",
  status: "active",
  display_name: "Test User",
  avatar_url: null,
  created_at: "2026-01-01T00:00:00Z",
};

export const MOCK_ADMIN = { ...MOCK_USER, role: "admin" };

export const MOCK_PERSONA = {
  id: "00000000-0000-7000-8000-000000000010",
  name: "Test Persona",
  relation: "self",
  description: null,
  birth_year: null,
  avatar_path: null,
  era_count: 0,
  doc_count: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

function json(route: Route, body: unknown, status = 200) {
  return route.fulfill({
    status,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
}

async function setCsrfCookie(page: Page) {
  await page.context().addCookies([
    {
      name: "pai_csrf",
      value: "test-csrf-token",
      domain: "localhost",
      path: "/",
    },
  ]);
}

// ─── Per-endpoint mocks ───────────────────────────────────────────────────────

/** Mock GET /api/auth/me returning the given user. Sets the CSRF cookie. */
export async function mockMe(page: Page, user = MOCK_USER) {
  await page.route("**/api/auth/me", (route) => {
    if (route.request().method() === "GET") return json(route, user);
    return route.fallback();
  });
  await setCsrfCookie(page);
}

/** Mock GET /api/auth/me returning 401 (unauthenticated). */
export async function mockUnauthenticated(page: Page) {
  await page.route("**/api/auth/me", (route) => {
    if (route.request().method() === "GET") {
      return route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ code: "unauthorized", message: "Not authenticated." }),
      });
    }
    return route.fallback();
  });
}

/**
 * Mock a stateful login flow: /me returns 401 until login succeeds,
 * then returns the user. Use this instead of combining mockUnauthenticated +
 * mockLogin when you need the post-login /me query to resolve correctly.
 */
export async function mockLoginFlow(page: Page, user = MOCK_USER) {
  let authenticated = false;

  await page.route("**/api/auth/me", (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    if (authenticated) return json(route, user);
    return route.fulfill({
      status: 401,
      contentType: "application/json",
      body: JSON.stringify({ code: "unauthorized", message: "Not authenticated." }),
    });
  });

  await page.route("**/api/auth/login", async (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    authenticated = true;
    await setCsrfCookie(page);
    return json(route, user);
  });
}

/** Mock POST /api/auth/logout → 204. */
export async function mockLogout(page: Page) {
  await page.route("**/api/auth/logout", (route) => {
    if (route.request().method() === "POST") return route.fulfill({ status: 204 });
    return route.fallback();
  });
}

/** Mock GET /api/personas (list, may include ?limit=N) and POST /api/personas (create). */
export async function mockPersonas(
  page: Page,
  items: typeof MOCK_PERSONA[] = [MOCK_PERSONA],
) {
  // Use a function matcher so query params like ?limit=200 are included correctly
  await page.route(
    (url) => url.pathname === "/api/personas",
    (route) => {
      if (route.request().method() === "GET") {
        return json(route, { items, next_cursor: null });
      }
      if (route.request().method() === "POST") {
        return json(route, MOCK_PERSONA, 201);
      }
      return route.fallback();
    },
  );
}

/** Mock GET /api/personas/:id and the persona workspace sub-routes. */
export async function mockPersonaWorkspace(
  page: Page,
  persona = MOCK_PERSONA,
) {
  await page.route(`**/api/personas/${persona.id}`, (route) => {
    if (route.request().method() === "GET") return json(route, persona);
    return route.fallback();
  });
  // Eras list — workspace sidebar calls this
  await page.route(`**/api/personas/${persona.id}/eras`, (route) => {
    if (route.request().method() === "GET") {
      return json(route, { items: [], next_cursor: null });
    }
    return route.fallback();
  });
  // Style profile — dashboard may call this
  await page.route(`**/api/personas/${persona.id}/profile`, (route) => {
    if (route.request().method() === "GET") {
      return route.fulfill({ status: 404, contentType: "application/json", body: JSON.stringify({ code: "not_found" }) });
    }
    return route.fallback();
  });
}
