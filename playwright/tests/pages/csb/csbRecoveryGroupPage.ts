 import { expect, type Locator, type Page } from "@playwright/test";
 
 export class CsbRecoveryGroupPage {
   readonly buttonBack: Locator;
   readonly buttonRecoverOmissions: Locator;
 
   constructor(protected readonly page: Page) {
     const headerButtons = this.page.locator(".header-buttons");
     this.buttonBack = headerButtons.getByRole("link", { name: "Terug" });
     this.buttonRecoverOmissions = headerButtons.getByRole("link", {
       name: "Verzuimen herstellen",
     });
   }

    async selectedGroup(politicalgroup: string) {
       await expect(
         this.page.getByRole("heading", { name: politicalgroup }),
       ).toBeVisible();
     }


async recoverOmission(district: string, recovered: boolean) {
  const buttonName = recovered ? 'Hersteld' : 'Niet hersteld';

  // Layout 1: table row with multiple districts
  const tableRow = this.page
    .getByRole('row')
    .filter({ has: this.page.getByRole('rowheader', { name: district }) });

  // Layout 2: single district list item 
  const singleItem = this.page
    .locator('li.omission-status-row')
    .filter({ has: this.page.locator('.omission-districts', { hasText: district }) });

  const button = tableRow.or(singleItem).getByRole('button', { name: buttonName, exact: true });

  await button.click();
  await expect(button).toHaveClass(/\bselected\b/);
}
  
 }

 
 
 
