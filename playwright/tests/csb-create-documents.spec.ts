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
    await hearingDetailsPage.setDateOfHearing("30", "11", "2027");
    await hearingDetailsPage.textfieldTimeOfHearing.fill("10:57");
    await hearingDetailsPage.textfieldNameOfChairperson.fill("Vera van Veen");
    await page.locator('#signer_0').fill('Jan Jansen');
    await page.locator('#signer_1').fill('Diede de Bruin');
    await hearingDetailsPage.buttonSave.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();  

    // download I 1 in pdf format
    const pdfPromise = page.waitForEvent("download");
    await finalizeExaminationPage.downloadI1pdf.click();
    const pdfDownload = await pdfPromise;
    expect(pdfDownload.suggestedFilename()).toMatch(/i1-proces-verbaal\.pdf/);
    expect((await stat(await pdfDownload.path())).size).toBeGreaterThan(1024);

    // download I 1 in word format
    const wordPromise = page.waitForEvent("download");
    await finalizeExaminationPage.downloadI1word.click();
    const wordDownload = await wordPromise;
    expect(wordDownload.suggestedFilename()).toMatch(/i1-proces-verbaal\.docx/);
    expect((await stat(await wordDownload.path())).size).toBeGreaterThan(1024);
    
  });


  

  });

