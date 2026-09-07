import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";
import { CsbOmissionsDeclarationsOfSupportPage } from "./pages/csb/csbOmissionsDeclarationsOfSupport.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test("finalize examination happy flow", async ({ csbImport }) => {
  const { page, groupName } = csbImport;
  const examinationPage = new CsbExaminationPage(page);
  const politicalGroupPage = new CsbPoliticalGroupPage(page);

  await page.goto("/csb/examination");
  await expect(page.getByText(`${groupName} Onderzoek bezig`)).toBeVisible();
  await examinationPage.selectPoliticalGroup(groupName);
  await expect(page.getByText("Onderzoek afronden")).toBeVisible();
  await expect(page.locator(".examination-panels")).not.toHaveClass(/disabled/);
  await politicalGroupPage.switchFinalize.click();
  await expect(page.getByText("Onderzoek afgerond")).toBeVisible();
  await expect(page.locator(".examination-panels")).toHaveClass(/disabled/);
  await politicalGroupPage.buttonBack.click();
  await expect(page.getByText(`${groupName} Goedgekeurd`)).toBeVisible();
});

test("finalize examination with omissions", async ({ csbImport }) => {
  const { page, groupName } = csbImport;
  const examinationPage = new CsbExaminationPage(page);
  const politicalGroupPage = new CsbPoliticalGroupPage(page);
  const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);

  await page.goto("/csb/examination");
  await expect(page.getByText(`${groupName} Onderzoek bezig`)).toBeVisible();

  await examinationPage.selectPoliticalGroup(groupName);
  await politicalGroupPage.linkSupportDeclarations.click();
  await page.waitForURL(/\/omission\//);

  await page.getByRole("checkbox", { name: "1. Groningen" }).check();
  await omissionsPage.buttonMissingAllDistricts.click();
  await expect(omissionsPage.checkboxRecoverable).toBeChecked();
  await omissionsPage.textfieldLetter.fill("Testtoevoeging");
  await omissionsPage.buttonAddAndClose.click();
  await expect(page.locator("form.overlay")).toBeHidden();

  await expect(page.getByText("Onderzoek afronden")).toBeVisible();
  await expect(page.locator(".examination-panels")).not.toHaveClass(/disabled/);
  await politicalGroupPage.switchFinalize.click();
  await expect(page.getByText("Onderzoek afgerond")).toBeVisible();
  await expect(page.locator(".examination-panels")).toHaveClass(/disabled/);
  await politicalGroupPage.buttonBack.click();
  await expect(
    page.getByText(`${groupName} Verzuimen toegevoegd`),
  ).toBeVisible();
});
