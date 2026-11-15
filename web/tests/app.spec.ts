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
