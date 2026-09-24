// Assign a dataset value to a field, tolerating a missing field or value.
function setValue(
  field: HTMLInputElement | HTMLTextAreaElement | null,
  value: string | undefined,
) {
  if (field) {
    field.value = value ?? "";
  }
}

// Only recoverable omissions reach the omission letter, so the letter note
// is read-only while the omission is marked irreparable.
function syncHelpText(
  helpText: HTMLTextAreaElement | null,
  recoverable: HTMLInputElement | null,
) {
  if (helpText && recoverable) {
    helpText.readOnly = !recoverable.checked;
  }
}

// Fill the omission description and help-text fields when a preset is clicked.
export default function omissionPreset() {
  const title = document.querySelector<HTMLInputElement>(
    "[data-omission-title]",
  );
  const description = document.querySelector<HTMLTextAreaElement>(
    "[data-omission-description]",
  );
  const helpText = document.querySelector<HTMLTextAreaElement>(
    "[data-omission-help-text]",
  );
  const recoverable = document.querySelector<HTMLInputElement>(
    "[data-omission-recoverable]",
  );
  const noLetterWarning = document.querySelector<HTMLElement>(
    "[data-omission-no-letter-warning]",
  );

  // Only warn on a manual uncheck, not when a preset is irreparable.
  recoverable?.addEventListener("change", () => {
    syncHelpText(helpText, recoverable);
    noLetterWarning?.classList.toggle("hidden", recoverable.checked);
  });
  syncHelpText(helpText, recoverable);

  document
    .querySelectorAll<HTMLButtonElement>("[data-omission-preset]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        setValue(title, button.dataset.title);
        noLetterWarning?.classList.add("hidden");
        setValue(description, button.dataset.description);
        setValue(helpText, button.dataset.helpText);
        if (recoverable) {
          recoverable.checked = button.dataset.recoverable !== "false";
          syncHelpText(helpText, recoverable);
        }
        description?.focus();
      });
    });
}
