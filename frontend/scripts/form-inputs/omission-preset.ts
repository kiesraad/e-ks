// Assign a dataset value to a field, tolerating a missing field or value.
function setValue(
  field: HTMLInputElement | HTMLTextAreaElement | null,
  value: string | undefined,
) {
  if (field) {
    field.value = value ?? "";
  }
}

// Return Dutch names of the currently checked district checkboxes
function selectedDistrictNames(): string[] {
  const names: string[] = [];
  document
    .querySelectorAll<HTMLInputElement>(
      'input[name="electoral_districts"]:checked',
    )
    .forEach((input) => {
      if (input.dataset.districtNl) {
        names.push(input.dataset.districtNl);
      }
    });
  return names;
}

function updatePlaceholderWarning(
  description: HTMLTextAreaElement,
  warning: HTMLElement,
) {
  warning.classList.toggle("hidden", !description.value.includes("{"));
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
  const warning = document.querySelector<HTMLElement>(
    "[data-omission-placeholder-warning]",
  );

  if (!description || !warning) {
    return;
  }

  description.addEventListener("input", () =>
    updatePlaceholderWarning(description, warning),
  );

  recoverable?.addEventListener("change", () =>
    syncHelpText(helpText, recoverable),
  );
  syncHelpText(helpText, recoverable);

  document
    .querySelectorAll<HTMLButtonElement>("[data-omission-preset]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        setValue(title, button.dataset.title);

        const districts = selectedDistrictNames();
        let desc = button.dataset.description ?? "";
        let help = button.dataset.helpText ?? "";
        if (districts.length === 1) {
          desc = desc.replace("{district}", districts[0]);
          help = help.replace("{district}", districts[0]);
        } else if (districts.length > 1) {
          const last = districts.at(-1);
          const rest = districts.slice(0, -1);
          desc = desc.replace("{districts}", `${rest.join(", ")} en ${last}`);
          help = help.replace("{districts}", `${rest.join(", ")} en ${last}`);
        }

        setValue(description, desc);
        updatePlaceholderWarning(description, warning);
        setValue(helpText, help);
        if (recoverable) {
          recoverable.checked = button.dataset.recoverable !== "false";
          syncHelpText(helpText, recoverable);
        }
        description.focus();
      });
    });
}
