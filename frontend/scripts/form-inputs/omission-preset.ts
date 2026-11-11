// Assign a dataset value to a field, tolerating a missing field or value.
// The change event lets stale server errors on the field clear themselves.
function setValue(
  field: HTMLInputElement | HTMLTextAreaElement | null,
  value: string | undefined,
) {
  if (field) {
    field.value = value ?? "";
    field.dispatchEvent(new Event("change"));
  }
}

type PresetText = { title: string; description: string; help_text: string };
type DistrictVariants = Record<"one" | "multiple" | "all", PresetText>;

const DISTRICTS = 'input[name="electoral_districts"]';

// The variant matching the selection: one, several, or all enabled districts.
// Nothing selected (or no selector) falls back to the one-district text.
function districtVariant(variants: DistrictVariants): PresetText {
  const checked = document.querySelectorAll(`${DISTRICTS}:checked`).length;
  const enabled = document.querySelectorAll(`${DISTRICTS}:enabled`).length;
  if (checked <= 1) {
    return variants.one;
  }
  return checked === enabled ? variants.all : variants.multiple;
}

// The Dutch names of the checked district checkboxes, listed as "A, B en C".
function selectedDistrictNames(): string | null {
  const names: string[] = [];
  document
    .querySelectorAll<HTMLInputElement>(`${DISTRICTS}:checked`)
    .forEach((input) => {
      if (input.dataset.districtNl) {
        names.push(input.dataset.districtNl);
      }
    });
  if (names.length === 0) {
    return null;
  }
  if (names.length === 1) {
    return names[0];
  }
  return `${names.slice(0, -1).join(", ")} en ${names.at(-1)}`;
}

// Replace the district tokens in a text with the selected district names.
// Without a selection the tokens stay, so the server rejects the submit.
function fillDistricts(text: string): string {
  const districts = selectedDistrictNames();
  if (districts === null) {
    return text;
  }
  return text
    .replaceAll("{district}", districts)
    .replaceAll("{districts}", districts);
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

  const fields = [title, description, helpText] as const;
  // The district variants of the last clicked preset, and the values it put in
  // the fields, so a new district selection only replaces untouched texts.
  let activeVariants: DistrictVariants | null = null;
  let filled: string[] = [];

  const fillVariant = (variants: DistrictVariants) => {
    const text = districtVariant(variants);
    const values = [
      text.title,
      fillDistricts(text.description),
      fillDistricts(text.help_text),
    ];
    fields.forEach((field, i) => {
      if (field && (filled.length === 0 || field.value === filled[i])) {
        setValue(field, values[i]);
      }
    });
    filled = values;
  };

  // Listening on the section catches the bubbled change of the individual
  // checkboxes and of their "select all" checkbox, after the latter has
  // updated the individual ones.
  document
    .querySelector(DISTRICTS)
    ?.closest(".omission-form-section")
    ?.addEventListener("change", () => {
      if (activeVariants) {
        fillVariant(activeVariants);
        return;
      }
      // Fill district tokens left behind when a preset was clicked before the
      // districts were selected.
      for (const field of [description, helpText]) {
        if (field) {
          field.value = fillDistricts(field.value);
        }
      }
    });

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
        noLetterWarning?.classList.add("hidden");
        const variants = button.dataset.districtVariants;
        activeVariants = variants ? JSON.parse(variants) : null;
        filled = [];
        if (activeVariants) {
          fillVariant(activeVariants);
        } else {
          setValue(title, button.dataset.title);
          setValue(
            description,
            fillDistricts(button.dataset.description ?? ""),
          );
          setValue(helpText, fillDistricts(button.dataset.helpText ?? ""));
        }
        if (recoverable) {
          recoverable.checked = button.dataset.recoverable !== "false";
          syncHelpText(helpText, recoverable);
        }
        description?.focus();
      });
    });
}
