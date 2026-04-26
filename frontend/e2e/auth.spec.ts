import { test, expect } from "@playwright/test";
import {
  mockMe,
  mockUnauthenticated,
  mockLoginFlow,
  mockLogout,
  mockPersonas,
  MOCK_USER,
} from "./helpers/mock-api";

test.describe("Auth: route guards", () => {
  test("unauthenticated user visiting /personas is redirected to /login", async ({
    page,
  }) => {
    await mockUnauthenticated(page);
    await page.goto("/personas");
    await expect(page).toHaveURL(/\/login/);
  });

  test("authenticated user visiting /login is redirected to /personas", async ({
    page,
  }) => {
    await mockMe(page);
    await mockPersonas(page);
    await page.goto("/login");
    await expect(page).toHaveURL(/\/personas/);
  });

  test("unauthenticated user visiting / is redirected to /login", async ({
    page,
  }) => {
    await mockUnauthenticated(page);
    await page.goto("/");
    await expect(page).toHaveURL(/\/login/);
  });
});

test.describe("Auth: login form", () => {
  test("happy path — fills form, submits, lands on /personas", async ({
    page,
  }) => {
    await mockLoginFlow(page);
    await mockPersonas(page);

    await page.goto("/login");
    await page.locator("input#email").fill(MOCK_USER.email);
    await page.locator("input#password").fill("password123");
    await page.getByRole("button", { name: "Sign in" }).click();

    await expect(page).toHaveURL(/\/personas/);
  });

  test("wrong credentials shows error message", async ({ page }) => {
    await mockUnauthenticated(page);
    await page.route("**/api/auth/login", (route) => {
      return route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({
          code: "invalid_credentials",
          message: "Invalid email or password.",
        }),
      });
    });

    await page.goto("/login");
    await page.locator("input#email").fill("wrong@example.com");
    await page.locator("input#password").fill("badpassword");
    await page.getByRole("button", { name: "Sign in" }).click();

    await expect(page.getByText("Invalid email or password.")).toBeVisible();
    await expect(page).toHaveURL(/\/login/);
  });

  test("empty email shows validation error without hitting API", async ({
    page,
  }) => {
    await mockUnauthenticated(page);

    await page.goto("/login");
    await page.locator("input#password").fill("password123");
    await page.getByRole("button", { name: "Sign in" }).click();

    await expect(
      page.getByText("Enter a valid email address."),
    ).toBeVisible();
  });

  test("empty password shows validation error without hitting API", async ({
    page,
  }) => {
    await mockUnauthenticated(page);

    await page.goto("/login");
    await page.locator("input#email").fill("test@example.com");
    await page.getByRole("button", { name: "Sign in" }).click();

    await expect(page.getByText("Password is required.")).toBeVisible();
  });
});

test.describe("Auth: sign out", () => {
  test("clicking Sign out redirects to /login", async ({ page }) => {
    await mockMe(page);
    await mockPersonas(page);
    await mockLogout(page);

    await page.goto("/personas");
    await expect(page.getByText("Persona AI")).toBeVisible();

    await page.getByRole("button", { name: "Sign out" }).click();

    await expect(page).toHaveURL(/\/login/);
  });
});
