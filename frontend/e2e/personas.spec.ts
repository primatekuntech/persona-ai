import { test, expect } from "@playwright/test";
import {
  mockMe,
  mockPersonas,
  mockPersonaWorkspace,
  MOCK_PERSONA,
} from "./helpers/mock-api";

test.describe("Personas: list", () => {
  test("renders persona cards from API", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, [MOCK_PERSONA]);

    await page.goto("/personas");

    await expect(page.getByText(MOCK_PERSONA.name)).toBeVisible();
  });

  test("empty state shows when no personas", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");

    // No persona cards — the create button should still be present
    await expect(page.getByRole("button", { name: /new persona/i })).toBeVisible();
    await expect(page.getByText(MOCK_PERSONA.name)).not.toBeVisible();
  });

  test("page heading and create button are present", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");

    await expect(
      page.getByRole("heading", { name: /personas/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: /new persona/i }),
    ).toBeVisible();
  });
});

test.describe("Personas: create dialog", () => {
  test("dialog opens on New persona button click", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await page.getByRole("button", { name: /new persona/i }).click();

    await expect(
      page.getByRole("heading", { name: "Create persona" }),
    ).toBeVisible();
    await expect(page.locator("input#cp-name")).toBeVisible();
  });

  test("create happy path — submits and navigates to workspace dashboard", async ({
    page,
  }) => {
    await mockMe(page);
    await mockPersonas(page, []);
    await mockPersonaWorkspace(page, MOCK_PERSONA);

    // Mock the documents endpoint the workspace may call
    await page.route(`**/api/personas/${MOCK_PERSONA.id}/documents`, (route) => {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: [], next_cursor: null }),
      });
    });

    await page.goto("/personas");
    await page.getByRole("button", { name: /new persona/i }).click();

    await page.locator("input#cp-name").fill("Alice, age 25");
    await page.getByRole("button", { name: /create/i }).click();

    await expect(page).toHaveURL(
      new RegExp(`/personas/${MOCK_PERSONA.id}/dashboard`),
    );
  });

  test("create dialog validates required name", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await page.getByRole("button", { name: /new persona/i }).click();

    // Submit without filling name
    await page.getByRole("button", { name: /create/i }).click();

    await expect(page.getByText("Required")).toBeVisible();
  });

  test("create dialog can be closed without submitting", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await page.getByRole("button", { name: /new persona/i }).click();
    await expect(
      page.getByRole("heading", { name: "Create persona" }),
    ).toBeVisible();

    // Press Escape to close
    await page.keyboard.press("Escape");

    await expect(
      page.getByRole("heading", { name: "Create persona" }),
    ).not.toBeVisible();
  });
});
