import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CsbRegisteredPoliticalGroupsPage } from "./pages/csb/csbRegisteredPoliticalGroupsPage.ts";

test("register, edit and delete a political group with its previous election result", async ({
  csbLogin,
}) => {
  const { page } = csbLogin;
  const overviewPage = new CsbOverviewPage(page);
  const groupsPage = new CsbRegisteredPoliticalGroupsPage(page);
  const appellation = `Lijst ${Math.random().toString(36).slice(2, 10)}`;
  const renamed = `${appellation} Nieuw`;

  await expect(overviewPage.headerElection).toBeVisible();
  await overviewPage.linkRegisteredPoliticalGroups.click();
  await expect(groupsPage.header).toBeVisible();

  // add a group
  await groupsPage.linkAdd.click();
  await expect(groupsPage.headerAdd).toBeVisible();
  await groupsPage.fillForm(appellation, "12345", "3");
  await groupsPage.save();
  const row = groupsPage.row(appellation);
  await expect(row).toBeVisible();
  await expect(row).toContainText("12345");

  // the same appellation cannot be registered twice
  await groupsPage.linkAdd.click();
  await groupsPage.fillForm(appellation.toLowerCase(), "1", "0");
  await groupsPage.buttonSave.click();
  await expect(
    page.getByText(
      "Er is al een politieke groepering met deze aanduiding geregistreerd.",
    ),
  ).toBeVisible();
  await page.getByRole("link", { name: "Sluiten" }).click();
  await expect(groupsPage.header).toBeVisible();

  // edit the group
  await row.getByRole("link", { name: "Bewerken" }).click();
  await expect(groupsPage.headerEdit).toBeVisible();
  await expect(groupsPage.textfieldAppellation).toHaveValue(appellation);
  await expect(groupsPage.textfieldVotes).toHaveValue("12345");
  await expect(groupsPage.textfieldSeats).toHaveValue("3");
  await groupsPage.fillForm(renamed, "54321", "4");
  await groupsPage.save();
  const renamedRow = groupsPage.row(renamed);
  await expect(renamedRow).toBeVisible();
  await expect(renamedRow).toContainText("54321");
  await expect(groupsPage.row(appellation)).toBeHidden();

  // delete the group
  await renamedRow.getByRole("link", { name: "Bewerken" }).click();
  await groupsPage.linkDelete.click();
  await expect(
    page.getByText(`Weet u zeker dat u de politieke groepering “${renamed}“`),
  ).toBeVisible();
  await Promise.all([
    page.waitForURL(/\/csb\/registered-political-groups\?/),
    groupsPage.buttonDeleteConfirm.click(),
  ]);
  await expect(renamedRow).toBeHidden();
});
