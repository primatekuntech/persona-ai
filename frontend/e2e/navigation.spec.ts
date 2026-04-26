import { test, expect } from "@playwright/test";
import {
  mockMe,
  mockPersonas,
  mockPersonaWorkspace,
  MOCK_USER,
  MOCK_ADMIN,
  MOCK_PERSONA,
} from "./helpers/mock-api";

test.describe("Navigation: sidebar", () => {
  test("sidebar shows Personas and Settings links for regular user", async ({
    page,
  }) => {
    await mockMe(page, MOCK_USER);
    await mockPersonas(page);

    await page.goto("/personas");

    await expect(page.getByRole("link", { name: "Personas" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Settings" })).toBeVisible();
  });

  test("admin section is hidden for regular user", async ({ page }) => {
    await mockMe(page, MOCK_USER);
    await mockPersonas(page);

    await page.goto("/personas");

    await expect(page.getByText("Admin")).not.toBeVisible();
    await expect(page.getByRole("link", { name: "Users" })).not.toBeVisible();
  });

  test("admin section is visible for admin user", async ({ page }) => {
    await mockMe(page, MOCK_ADMIN);
    await mockPersonas(page);

    await page.goto("/personas");

    await expect(page.getByText("Admin")).toBeVisible();
    await expect(page.getByRole("link", { name: "Users" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Invites" })).toBeVisible();
  });

  test("Settings link navigates to /settings/account", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page);

    // Stub Settings page API calls
    await page.route("**/api/auth/me", (route) => {
      if (route.request().method() === "GET") {
        return route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(MOCK_USER),
        });
      }
      return route.fallback();
    });

    await page.goto("/personas");
    await page.getByRole("link", { name: "Settings" }).click();

    await expect(page).toHaveURL(/\/settings\/account/);
  });

  test("sidebar shows user display name and email", async ({ page }) => {
    await mockMe(page, MOCK_USER);
    await mockPersonas(page);

    await page.goto("/personas");

    await expect(page.getByText(MOCK_USER.display_name!)).toBeVisible();
    await expect(page.getByText(MOCK_USER.email)).toBeVisible();
  });
});

test.describe("Navigation: access control", () => {
  test("regular user visiting /admin/users is redirected to /personas", async ({
    page,
  }) => {
    await mockMe(page, MOCK_USER);
    await mockPersonas(page);

    await page.goto("/admin/users");

    await expect(page).toHaveURL(/\/personas/);
  });

  test("admin user can access /admin/users", async ({ page }) => {
    await mockMe(page, MOCK_ADMIN);
    await mockPersonas(page);

    // Stub admin users API
    await page.route("**/api/admin/users*", (route) => {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: [], next_cursor: null }),
      });
    });

    await page.goto("/admin/users");

    await expect(page).toHaveURL(/\/admin\/users/);
  });
});

test.describe("Navigation: persona workspace", () => {
  test("persona workspace sub-routes render without redirect", async ({
    page,
  }) => {
    await mockMe(page);
    await mockPersonaWorkspace(page, MOCK_PERSONA);
    await mockPersonas(page, [MOCK_PERSONA]);

    // Stub documents list
    await page.route(`**/api/personas/${MOCK_PERSONA.id}/documents*`, (route) => {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: [], next_cursor: null }),
      });
    });

    await page.goto(`/personas/${MOCK_PERSONA.id}/documents`);

    await expect(page).toHaveURL(
      new RegExp(`/personas/${MOCK_PERSONA.id}/documents`),
    );
  });

  test("/personas/:id redirects to /personas/:id/dashboard", async ({
    page,
  }) => {
    await mockMe(page);
    await mockPersonaWorkspace(page, MOCK_PERSONA);

    await page.goto(`/personas/${MOCK_PERSONA.id}`);

    await expect(page).toHaveURL(
      new RegExp(`/personas/${MOCK_PERSONA.id}/dashboard`),
    );
  });
});
