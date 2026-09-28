// Use cases:
// CSB maakt een proces-verbaal van het onderzoek naar de kandidatenlijsten (model I 1) aan zonder verzuimen.
// CSB maakt een proces-verbaal van het onderzoek naar de kandidatenlijsten aan (model I1)
// CSB maakt een verzuimbrief per partij aan
// CSB stelt de definitieve kandidatatenlijsten vast in een proces-verbaal over geldigheid en nummering kandidatenlijsten (model I4)
// CSB maakt een concept proces-verbaal model I4 aan in e-KS

import { stat } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbHearingDetailsPage } from "./pages/csb/csbHearingDetailsPage.ts";
import { CsbFinalizeExaminationPage } from "./pages/csb/csbFinalizeExaminationPage.ts";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";

test.describe("create documents", async () => {

   test("I 1", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const finalizeExaminationPage = new CsbFinalizeExaminationPage(page);
    const hearingDetailsPage = new CsbHearingDetailsPage(page);
    const examinationPage = new CsbExaminationPage(page);
    
    await page.goto("/csb/examination");
    await examinationPage.buttonFinalise.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();

    // add hearing details
    await finalizeExaminationPage.linkHearingDetails.click();
    await expect(hearingDetailsPage.headerHearing).toBeVisible();
    await hearingDetailsPage.textfieldDateOfHearing.fill("01-01-2024");
    await hearingDetailsPage.textfieldTimeOfHearing.fill("10:00");
    await hearingDetailsPage.textfieldNameOfChairperson.fill("Vera van Veen");
    await page.locator('#signer_0').fill('Jan Jansen');
    await page.locator('#signer_1').fill('Diede de Bruin');
    await hearingDetailsPage.buttonSave.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();  

    //
    // const downloadPromise = page.waitForEvent("download");
    // await new FinalisePage(page).linkDownloadNl.click();
    // const download = await downloadPromise;

    // expect(download.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+\.zip$/);
    // expect((await stat(await download.path())).size).toBeGreaterThan(1024);

    
  });
  });

