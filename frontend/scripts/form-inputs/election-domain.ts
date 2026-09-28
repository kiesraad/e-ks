const FORM_SELECTOR = ".election-switch";

/**
 * Shows the domain select matching the chosen election's domain kind,
 * hides the others.
 */
function updateDomainVisibility(form: HTMLFormElement) {
  const select = form.querySelector<HTMLSelectElement>(
    'select[name="election"]',
  );
  const selected = select?.selectedOptions[0];
  const kind = selected?.dataset.domainKind ?? "";

  form.querySelectorAll<HTMLElement>(".domain-select").forEach((el) => {
    el.style.display = el.dataset.domainKind === kind ? "" : "none";
  });
}

export default function electionDomain() {
  document.querySelectorAll<HTMLFormElement>(FORM_SELECTOR).forEach((form) => {
    updateDomainVisibility(form);

    const select = form.querySelector('select[name="election"]');
    select?.addEventListener("change", () => updateDomainVisibility(form));
  });
}
