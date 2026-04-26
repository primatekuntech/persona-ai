import { test, expect } from "@playwright/test";
import {
  mockMe,
  mockPersonas,
  mockPersonaWorkspace,
  MOCK_PERSONA,
} from "./helpers/mock-api";

// ─── Helper: click the header "Create persona" button ────────────────────────
// Before the dialog opens there is exactly one such button. After it opens
// there are two (header + dialog submit), so we always click header via .first().
function clickCreateButton(page: Parameters<typeof mockMe>[0]) {
  return page.getByRole("button", { name: "Create persona" }).first().click();
}

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

    await expect(
      page.getByRole("button", { name: "Create persona" }).first(),
    ).toBeVisible();
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
      page.getByRole("button", { name: "Create persona" }).first(),
    ).toBeVisible();
  });
});

test.describe("Personas: create dialog", () => {
  test("dialog opens on Create persona button click", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await clickCreateButton(page);

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

    await page.route(`**/api/personas/${MOCK_PERSONA.id}/documents`, (route) => {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ items: [], next_cursor: null }),
      });
    });

    await page.goto("/personas");
    await clickCreateButton(page);

    await page.locator("input#cp-name").fill("Alice, age 25");
    // Click the submit button inside the dialog (not the header button)
    await page.getByRole("dialog").getByRole("button", { name: "Create persona" }).click();

    await expect(page).toHaveURL(
      new RegExp(`/personas/${MOCK_PERSONA.id}/dashboard`),
    );
  });

  test("create dialog validates required name", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await clickCreateButton(page);

    await page.getByRole("dialog").getByRole("button", { name: "Create persona" }).click();

    await expect(page.getByText("Required")).toBeVisible();
  });

  test("create dialog can be closed without submitting", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page, []);

    await page.goto("/personas");
    await clickCreateButton(page);
    await expect(
      page.getByRole("heading", { name: "Create persona" }),
    ).toBeVisible();

    await page.keyboard.press("Escape");

    await expect(
      page.getByRole("heading", { name: "Create persona" }),
    ).not.toBeVisible();
  });
});
