import { test, expect } from "@playwright/test";
test("live arena, mobile layout, replay and participant workshop", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.setViewportSize({ width: 1440, height: 1080 });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Every card has a price." }),
  ).toBeVisible();
  await expect(
    page.getByText("LIVE MATCH", { exact: false }).first(),
  ).toBeVisible();
  await expect(page.locator(".seat")).toHaveCount(6);
  await expect(page.locator(".seat .rack").first()).toBeVisible();
  await page.screenshot({
    path: "test-results/arena-desktop.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "Shuffle integrity" }).click();
  await expect(page.getByText("SEED COMMITMENT")).toBeVisible();
  await page.getByRole("link", { name: "Build a bot", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Turn code into a contender." }),
  ).toBeVisible();
  await page.getByRole("link", { name: "04 · The economy" }).click();
  await expect(
    page.getByRole("heading", { name: "04 / Price the possibilities" }),
  ).toBeVisible();
  expect(new URL(page.url()).hash).toBe("#docs");
  await page.getByRole("button", { name: "My workshop" }).click();
  await page
    .getByLabel("Team name", { exact: true })
    .fill(`Browser Team ${Date.now()}`);
  await page.getByRole("button", { name: "Create your team" }).click();
  await expect(
    page.getByRole("heading", { name: "Owner credential", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Version name").fill("browser-v1");
  await page.getByRole("button", { name: "Create bot version" }).click();
  await expect(
    page.getByRole("heading", { name: "Bot credential", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("cell", { name: "browser-v1" })).toBeVisible();
  await page.getByRole("button", { name: "Lock workshop" }).click();
  await expect(
    page.getByLabel("Owner credential", { exact: true }),
  ).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("link", { name: "The arena", exact: true }).click();
  await expect(page.locator(".seat")).toHaveCount(6);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBeTruthy();
  await page.screenshot({
    path: "test-results/arena-mobile.png",
    fullPage: true,
  });
  await page.getByRole("link", { name: "Rankings", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "The leaderboard." }),
  ).toBeVisible();
  await page
    .locator(".contender")
    .filter({ hasText: "browser-v1" })
    .last()
    .getByRole("button", { name: "browser-v1", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Contender history" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "All team versions", exact: true })
    .click();
  await expect(
    page.getByText("No matches recorded yet.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("link", { name: "Tournaments", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Play for the table." }),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

test("completed matches expose a working replay timeline and seed audit", async ({
  page,
  request,
}) => {
  test.setTimeout(70000);
  let id = "";
  await expect
    .poll(
      async () => {
        const response = await request.get("/api/matches");
        const result = await response.json();
        id =
          result.result.matches.find(
            (m: { status: string; id: string }) => m.status === "complete",
          )?.id ?? "";
        return id;
      },
      { timeout: 60000, intervals: [250, 1000] },
    )
    .not.toBe("");
  await page.goto("/");
  const row = page
    .locator("tr")
    .filter({ hasText: `#${id.slice(0, 6).toUpperCase()}` });
  await row.getByRole("button").click();
  const slider = page.getByRole("slider", { name: "Draft timeline" });
  await expect(slider).toBeVisible();
  await slider.fill("0");
  await expect(
    page.getByText("CARD 1 / 52", { exact: false }).first(),
  ).toBeVisible();
  await page.getByRole("button", { name: "Play replay", exact: true }).click();
  await expect.poll(() => slider.inputValue()).not.toBe("0");
  await page.getByRole("button", { name: "Pause replay", exact: true }).click();
  await slider.fill((await slider.getAttribute("max")) ?? "0");
  await page.getByRole("button", { name: "Shuffle integrity" }).click();
  await expect(page.getByText("REVEALED SEED", { exact: true })).toBeVisible();
});

test("public paging reaches later roster and rankings pages", async ({
  page,
}) => {
  const bot = (i: number) => ({
    id: `fixture-${i}`,
    team_id: `team-${i}`,
    team: `Paging Team ${i}`,
    name: `Paging Bot ${i}`,
    rating: 1600 - i,
    games: 10,
    disabled: false,
    created_ms: 0,
  });
  for (const kind of ["bots", "leaderboard"]) {
    await page.route(`**/api/${kind}?*`, async (route) => {
      const offset = Number(
        new URL(route.request().url()).searchParams.get("offset") || 0,
      );
      const bots = Array.from(
        { length: Math.max(0, Math.min(100, 101 - offset)) },
        (_, i) => bot(offset + i),
      );
      await route.fulfill({
        json: {
          ok: true,
          result: {
            bots,
            total: 101,
            offset,
            next_offset: offset === 0 ? 100 : null,
          },
        },
      });
    });
  }
  await page.goto("/#rankings");
  await expect(page.locator(".contender")).toHaveCount(100);
  await page
    .getByRole("button", { name: "Next roster page", exact: true })
    .click();
  await expect(page.locator(".contender")).toHaveCount(1);
  await expect(page.locator(".contender")).toContainText("Paging Bot 100");
  await expect(
    page.getByRole("button", { name: "Next roster page", exact: true }),
  ).toBeDisabled();
  await page
    .getByRole("button", { name: "Next rankings page", exact: true })
    .click();
  await expect(page.locator("table").first().locator("tbody tr")).toHaveCount(1);
  await page
    .getByRole("button", { name: "Previous roster page", exact: true })
    .click();
  await expect(page.locator(".contender")).toHaveCount(100);
  await expect(page.locator("table").first().locator("tbody tr")).toHaveCount(1);
});
