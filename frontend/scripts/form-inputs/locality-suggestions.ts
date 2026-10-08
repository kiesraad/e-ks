// Fetches and shows a single clickable locality suggestion under the input
// when the user's value doesn't exactly match a known Dutch locality. The
// suggestion is rendered by clicking the element with id `locality-suggestion-name`,
// which on click overwrites the input value and clears any warning state.
//
// The `/suggest` endpoint returns a plain JSON array of suggestion names. A
// name already carries a province suffix (e.g. "Bergen (LI)") when the source
// disambiguated a duplicate place name. Frisian aliases, when requested, are
// returned as names in their own right, so a user typing an alias gets an
// exact match without any special handling here.

import { isNetherlands } from "./country-code";

export default function localitySuggestions() {
  const input = document.getElementById("locality") as HTMLInputElement | null;
  const suggestion = document.getElementById("locality-suggestion");
  const suggestionName = document.getElementById("locality-suggestion-name");
  const localityWarning = document.getElementById(
    "unknown-locality",
  ) as HTMLElement | null;

  if (!input || !suggestion || !suggestionName) {
    return;
  }

  const field = input.closest(".form-field");

  // The template opts in to municipality results by adding `with-municipalities`
  // to the suggestion element. Forms that only accept actual places leave it off.
  const withMunicipalities = suggestion.classList.contains(
    "with-municipalities",
  );

  // Frisian aliases are only offered as suggestions when the template adds
  // `frisian-aliases-allowed` to the suggestion element (Frisian export forms).
  const withAliases = suggestion.classList.contains("frisian-aliases-allowed");

  const countryInput = document.getElementById(
    "country",
  ) as HTMLInputElement | null;
  const isNl = () => isNetherlands(countryInput);

  // `warn` distinguishes "user is actively typing" (false) from "user has
  // committed by blurring, or the form just loaded with a prefilled value"
  // (true). While typing we never add a warning — only remove one if it
  // turns out the value is now valid — to avoid flashing red on every keystroke.
  const runUpdate = async (warn: boolean) => {
    if (!isNl()) {
      suggestion.classList.add("hidden");
      localityWarning?.classList.add("hidden");
      field?.classList.remove("warning");
      return;
    }

    const q = input.value;

    // Below 3 characters the backend won't return useful results; treat
    // a too-short value as a warning on blur but stay silent while typing.
    if (q.length < 3) {
      suggestion.classList.add("hidden");
      return;
    }

    const url =
      `/suggest?wp=${encodeURIComponent(q)}` +
      `&municipalities=${withMunicipalities}` +
      `&aliases=${withAliases}` +
      `&limit=1`;

    const res = await fetch(url);
    const suggestions: Array<string> = await res.json();

    // A value is valid when it matches one of the returned suggestion names
    // (which already include any province suffix and requested aliases).
    const exactMatch = suggestions.some(
      (s) => s.toLowerCase() === q.toLowerCase(),
    );

    if (warn) {
      field?.classList.toggle("warning", !exactMatch);
    } else if (exactMatch) {
      // Typing path: never add a warning here, but do clear one the moment
      // the value becomes valid so the user gets immediate positive feedback.
      field?.classList.remove("warning", "error");
    }

    if (exactMatch) {
      suggestion.classList.add("hidden");
      localityWarning?.classList.add("hidden");
      return;
    }

    const name = suggestions[0] ?? "";
    if (!name) {
      suggestion.classList.add("hidden");
      return;
    }

    suggestionName.textContent = name;
    suggestion.classList.remove("hidden");
    localityWarning?.classList.remove("hidden");
  };

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  const update = (warn: boolean) => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      void runUpdate(warn);
    }, 300);
  };

  // Accept the suggestion: copy the displayed name into the input, clear
  // the warning, and dispatch a change event so downstream listeners
  // (validators, address lookup) see the corrected value.
  suggestionName.addEventListener("click", () => {
    input.value = suggestionName.textContent ?? "";
    suggestion.classList.add("hidden");
    localityWarning?.classList.add("hidden");
    field?.classList.remove("warning", "error");
    input.dispatchEvent(new Event("change", { bubbles: true }));
    runUpdate(true);
  });

  input.addEventListener("input", () => update(false));
  input.addEventListener("blur", () => update(true));
  countryInput?.addEventListener("change", () => update(true));

  // Initial pass: validate any prefilled value (e.g. when the form is
  // re-rendered with server-side state) and surface a warning if invalid.
  update(true);
}
